# FORGE-614 — Add `scheduled_task_id` filter to `GET /v2/chats/`

## Summary of decisions (settled during investigation, do not re-litigate)

1. **Ordering: keep the existing `COALESCE(latest_response.created_at, chat.created_at) DESC`**, do **not** switch to raw `created_at DESC`. The ticket asked for `created_at DESC`, but that would change behaviour for every existing caller of this endpoint. For scheduled chats the two orderings agree in practice (a scheduled chat is created and immediately gets its first response). Document it as "most recent activity first" and add a deterministic tiebreak.
2. **Predicate: `AND ($5::uuid IS NULL OR chat.metadata->>'scheduled_task_id' = $5::text)`** — exactly as proposed in the ticket. Verified against a real Postgres via asyncpg 0.31.0 (see `.saga/artifacts/sql-before-after-postgres-probe.md`). Rejected the alternative `(chat.metadata->>'scheduled_task_id')::uuid = $5::uuid` because it casts *column data* and one malformed metadata value would raise `22P02` at query time.
3. **`limit`: keep default `10000`, add `Query(ge=1, le=10000)`; `skip`: add `Query(ge=0)`.** Following the existing precedent at `apps/api/src/api/routes/v2/interactions_route.py:196-197`. Lowering the default is **not** allowed — the team-wide FE consumer (`ScheduledFlowsStateService`) passes no `limit` and would silently start truncating.
4. **No new index.** `chats.metadata` has no index today; the query is already anchored on `ix_chats_entity_id` and per-entity scheduled-chat volume is low. An expression index is a separate perf ticket if it ever matters.
5. **No FE / `cx-cli` / `ws-ai-mcp` changes.** Confirmed `apps/ws-ai-mcp/.../olly_client.py:163-171` calls `GET /chats` with no params → unaffected. `frontend` and `cx-cli` are separate repos, not in this worktree, and out of scope per the ticket.

## Before-state (reproduced — read this, it is the whole justification)

The environment has **no docker, no local Postgres, no `just`/`dotenvx`, no `.env`, and no generated protobufs** (`common.generated.*` requires `just common::proto` → SSH to Coralogix proto repos). Consequently `import api.main` fails, so the API server, `just openapi-generate`, and the integration suite (whose session-scoped `test_environment` fixture shells out to `just tests-environment-up` → docker compose) **cannot be run here**. `uv sync --frozen` does work.

So the bug was reproduced by running **the literal SQL from `chat_repository.py:48-64`** against an embedded Postgres 16 with a minimal `team_chats`/`team_responses` schema:

- Fixture: 7 `type='scheduled'` chats for task T1, 3 for task T2 with `created_at` deliberately interleaved, plus a `slack` chat and a `web` chat with `metadata IS NULL`.
- **BEFORE**, `chat_type=scheduled&limit=5` → `[t1-run-6, t1-run-5, t1-run-4, t1-run-3, t2-run-2]` — a caller wanting T1's 5 newest runs gets only **4**, and cannot tell rows are missing. This is the bug.
- **AFTER** (proposed predicate), `scheduled_task_id=T1&limit=5` → `[t1-run-6 … t1-run-2]` — all five, correct, newest first.
- **AFTER** with the param omitted (`$5 = NULL`) → identical to BEFORE, confirming the change is genuinely additive.
- Rows whose `metadata` is `NULL` or is a Slack/Teams payload yield SQL `NULL` from `->>`, so `= $5` is `NULL` → excluded. **No extra guard needed.**

Full artifact (both probes, all variants, blocker table): `.saga/artifacts/sql-before-after-postgres-probe.md`.

## Changes, in dependency order

### 1. `apps/api/src/api/repositories/chat_repository.py` — `list_chats` (lines 29-69)

- Add trailing param `scheduled_task_id: uuid.UUID | None = None`.
- Add to the `WHERE` clause, immediately after the existing `AND ($4::text IS NULL OR chat.type = $4)` line and **before** `ORDER BY`/`OFFSET`/`LIMIT`:
  ```
  AND ($5::uuid IS NULL OR chat.metadata->>'scheduled_task_id' = $5::text)
  ```
- Change the `ORDER BY` to make it total/deterministic (currently only the `COALESCE(...)` expression, so ties are arbitrary and `skip`/`limit` pagination is unstable):
  ```
  ORDER BY COALESCE(latest_response.created_at, chat.created_at) DESC, chat.created_at DESC, chat.id DESC
  ```
  This only reorders rows that already tie, so it is not a behaviour change for any caller with distinct activity timestamps.
- Append `scheduled_task_id` as the 5th positional arg to `db.fetch(...)`. **Pass the `uuid.UUID` object directly** — do not `str()` it. (Verified: asyncpg binds a `uuid.UUID` correctly against the mixed `$5::uuid` / `$5::text` casts.)
- Update the docstring: document the new arg and state the ordering guarantee explicitly ("sorted by most recent activity — latest response, falling back to chat creation time — descending").

### 2. `apps/api/src/api/services/chat_service.py` — `list_chats` (lines 52-60)

Pure passthrough. Add `scheduled_task_id: uuid.UUID | None = None` and forward it. `uuid` is already imported. Prefer switching the repository call to keyword args while you are here so the positional 5th arg cannot be mis-ordered.

### 3. `apps/api/src/api/routes/v2/chats_route.py` — `list_chats` (lines 61-82)

- New signature (note `Query` is already imported; `Annotated` is already imported):
  ```python
  skip: Annotated[int, Query(ge=0)] = 0,
  limit: Annotated[int, Query(ge=1, le=10000)] = 10000,
  chat_type: ChatType | None = None,
  scheduled_task_id: uuid.UUID | None = None,
  ```
- Forward `scheduled_task_id` to `chat_service.list_chats`.
- Extend the docstring (this is the OpenAPI description — it is generated from here, and per `.claude/rules/backend-guide.md` the docstring is the contract):
  - `skip`: "Number of chats to skip for pagination (>= 0)."
  - `limit`: "Maximum number of chats to return. Default 10000, maximum 10000."
  - `scheduled_task_id`: "Optional filter — return only chats produced by this scheduled task (matched on `metadata.scheduled_task_id`). Applied in SQL before pagination, so `limit` yields the N most recent chats *for that task*. Intended to be combined with `chat_type=scheduled`."
  - Add an explicit ordering sentence: "Results are ordered by most recent activity first (latest response timestamp, falling back to the chat's creation time), then by creation time and id as deterministic tiebreaks."

### 4. Tests — `tests/integration/test_chats_route_v2.py` (new tests in the `GET /chats (list_chats)` section, ~line 1137+)

Follow the existing style in that file exactly: raw `INSERT INTO chats (id, entity_id, team_id, title, type, shared_options, metadata)` via the `db_connection` fixture, `v2_client.get(...)`, assertions, and a `try/finally` that deletes the inserted rows.

- `test_v2_list_chats__filters_by_scheduled_task_id` — two tasks T1/T2 with scheduled chats each, plus one `web` chat with `metadata = NULL` and one `slack` chat whose metadata has no `scheduled_task_id` key. Assert `?chat_type=scheduled&scheduled_task_id=<T1>` returns exactly T1's chats and none of the others. **This covers the NULL-metadata / wrong-metadata-shape edge cases — do not skip those two rows.**
- `test_v2_list_chats__scheduled_task_filter_applied_before_limit` — **the regression test for the actual bug.** Insert T1 and T2 chats with interleaved `created_at` such that the newest 5 rows of the unfiltered set contain only 4 T1 rows (mirror the probe fixture: T1 at `base + i hours` for i=0..6, T2 at `base + i hours + 30min` for i=0..2). Assert `?chat_type=scheduled&scheduled_task_id=<T1>&limit=5` returns **5** rows, all T1. Assert the same request *without* `scheduled_task_id` returns a list that includes a T2 chat — i.e. prove the test would fail against the old code.
- `test_v2_list_chats__ordering_is_most_recent_activity_first` — insert with deliberately out-of-order `created_at` values (not insertion order), and give one older chat a newer `team_responses` row so the `COALESCE` branch is actually exercised. Assert the returned order matches the documented "most recent activity first" guarantee. Use the existing helpers/patterns for inserting into `responses` in this file.
- `test_v2_list_chats__unknown_scheduled_task_id_returns_empty` — a random UUID returns `200` with `[]`.
- `test_v2_list_chats__invalid_scheduled_task_id_returns_422` — `?scheduled_task_id=not-a-uuid` → `422`.
- `test_v2_list_chats__limit_bounds` — `?limit=0` → `422`, `?limit=10001` → `422`, `?limit=10000` → `200`, `?skip=-1` → `422`.
- **Regression:** the three existing tests (`test_v2_list_chats__filters_by_type`, `__returns_all_when_chat_type_none`, `__excludes_note_chats`) and `tests/integration/test_scheduled_tasks_route.py::test_list_chats__web_filter_excludes_scheduled` must pass **unmodified**. If any needs editing, the change is not backward-compatible — stop and reconsider.

### 5. Regenerate the OpenAPI schema if the repo tracks it

`just openapi-generate` (root) / `apps/api: just openapi-generate`. Check whether a generated schema file is committed in git; if it is, regenerate and commit it, since this changes the public contract of `/v2/chats/`.

## Edge cases & risks

- **asyncpg param typing on the doubly-cast `$5`.** The single highest-risk detail (fails only at runtime, not at lint/mypy). Already validated against a real Postgres with asyncpg — see the artifact. Do not "simplify" the casts without re-validating.
- **`limit` cap is a wire-contract tightening.** A caller passing `limit > 10000` gets `422` where it previously succeeded. `frontend` and `cx-cli` are not in this worktree so this could not be verified by reading. Judged safe because `10000` is the current default and no in-repo caller passes `limit` at all. If the reviewer objects, the fallback is to document default/max in the docstring only and drop `le=`; success criterion #4 explicitly permits documenting the absence of a cap instead of adding one.
- **`scheduled_task_id` is not restricted to `chat_type=scheduled`.** Intentional: the predicate is on `metadata`, so passing it with another `chat_type` simply yields `[]`. Do not add a 400 for the combination.
- **Ordering tiebreak.** Adding `chat.created_at DESC, chat.id DESC` only affects already-tied rows. Do not remove the `COALESCE` expression — that is the load-bearing part existing callers depend on.
- **Don't touch the scheduled-task-runs endpoint.** Adding `title` to `ScheduledTaskHistory` is explicitly out of scope (rejected in the ticket as a breaking response-model change).
- **Layer discipline** (`.claude/rules/backend-guide.md`): SQL stays in the repository; the route stays thin; `chat_service.list_chats` remains a passthrough (do not add logic there).

## How to verify

Run from the repo root:

```bash
just lint                                   # or: just api::lint ; just common::lint ; just tests::lint
just test-api                               # API unit tests
just test-integration integration/test_chats_route_v2.py
just test-integration integration/test_scheduled_tasks_route.py
```

Then the full gate per `.claude/rules/verification.md`: `/code-checks` (lint + UT + integration in parallel).

**Prerequisites that are missing in the planning environment and must be present in the implementation environment:** docker/colima, `just`, `dotenvx`, a populated `.env` (`just setup-env-vars`), and generated protos (`just common::proto`, needs SSH to the Coralogix proto repos). If integration tests still cannot be run, say so explicitly rather than claiming verification — do not mark this done on lint alone.

**Behaviour to observe before vs. after** (this is the acceptance demo; capture it as an artifact):

```bash
# BEFORE — returns a mix of tasks; the requested task gets fewer than `limit` rows
curl -s '.../v2/chats/?chat_type=scheduled&limit=5' | jq '[.[] | {title, task: .metadata.scheduled_task_id}]'

# AFTER — exactly the 5 newest chats for that one task
curl -s '.../v2/chats/?chat_type=scheduled&scheduled_task_id=<TASK_ID>&limit=5' | jq '[.[] | {title, created_at, task: .metadata.scheduled_task_id}]'

# AFTER — omitting the param must be byte-identical to BEFORE
curl -s '.../v2/chats/?chat_type=scheduled&limit=5'
```

Save request/response JSON to `.saga/artifacts/`. The reproducible SQL-level before/after already lives at `.saga/artifacts/sql-before-after-postgres-probe.md` and can be re-run with the two scripts described there if the full stack is unavailable.

## Definition of done

- `scheduled_task_id` is an optional query param on `GET /v2/chats/`, threaded route → service → repository.
- The filter is in the SQL `WHERE` clause of the same statement as `OFFSET`/`LIMIT` — **not** a post-fetch Python filter or slice.
- Ordering is deterministic and documented in the route docstring.
- `limit` default (`10000`) and maximum are documented and enforced; `skip` is `>= 0`.
- New integration tests cover: task filtering, filter-before-limit (the actual bug), ordering, unknown id → `[]`, malformed id → `422`, limit bounds.
- All pre-existing `list_chats` tests pass **unmodified**.
