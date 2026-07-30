# FORGE-626 — DB constraint: one `watch_dashboard` task per (user, dashboard)

## Environment blocker (read first)

**The project could not be run in the planning sandbox.** Artifact: `.saga/artifacts/cli-before-state-and-blocker.md`.

- `docker` not installed → no Postgres/Redis/Restate → `just dev`, `just db-migrate`, `just test-integration` all unavailable.
- `just` / `dotenvx` not installed (only `uv` on PATH); no `.env` (needs AWS/Okta via `just setup-env-vars`).
- `uv sync` fails: **`No space left on device`** (`/` is 58G/58G, 13M free). Without a venv even unit tests can't collect (`ModuleNotFoundError: No module named 'common'`).

Before-state was therefore established **statically** (see artifact): `scheduled_tasks` has indexes `ix_scheduled_tasks_team_id`, `ix_scheduled_tasks_entity_id`, `ix_scheduled_tasks_next_run_at`, `ix_scheduled_tasks_enabled_next_run_at` (V28) and `uq_scheduled_tasks_team_entity_title` (V29). `grep -rn "dashboard_id"` across both migration dirs returns **zero matches** — nothing today prevents two `watch_dashboard` rows with the same `entity_id` + `metadata->>'dashboard_id'` as long as titles differ. **The implementation step must run the verification commands in §7 on a machine with Docker + a synced venv.**

## Decisions taken (flag in-thread if you disagree)

I raised two questions and got no answer before writing this, so I took the conservative option for each:

1. **Index includes `team_id`** → `(team_id, entity_id, (metadata->>'dashboard_id'))`, not the bare `(entity_id, dashboard_id)` the ticket sketches. Reasons: it matches both existing precedents on this schema (`uq_scheduled_tasks_team_entity_title` at `V29__scheduled_task_table_fixes.sql:9`, and the partial expression indexes `uq_skills_user`/`uq_skills_team` in `78ab0715669f_skills_unique_normalized_name.py`); `scheduled_tasks` is a team-partitioned table read exclusively through the `team_scheduled_tasks` RLS view; dashboards are team-scoped so a dashboard_id never legitimately spans teams; and a `team_id`-prefixed index is directly usable by `list_scheduled_tasks_for_dashboard` (`scheduled_task_repository.py:59-78`), which always filters `team_id` (via the view) + `entity_id` + `kind` + `dashboard_id`. It does **not** over-scope (entity_id is still in the key), and it cannot cause a surprise cross-team collision for a user who belongs to several teams. Index name: **`uq_scheduled_tasks_team_entity_dashboard`**.
2. **Pre-existing duplicates: plain `CREATE UNIQUE INDEX`, fail loudly.** Dedup is explicitly out of scope per the ticket. **Ops precondition before merge:** run the query in §6 against every deployed environment; if it returns rows, stop and open a separate cleanup ticket — otherwise the deploy will fail at migration time.
3. **Agent path gets a graceful `ToolError`** (not a re-enable-existing-task behaviour, which would be new product scope). See §4.

## 1. Alembic migration (do this first — everything else depends on the index name)

Create it with the repo command so `down_revision` is wired automatically:

```bash
just db-create-migration "scheduled_tasks_unique_watch_dashboard_per_entity"
```

Expected `down_revision` = **`d3f2a91b6e04`** (V48, `migrate_scheduled_tasks_to_gpt_5_6_terra`) — confirmed current head: no revision file declares it as `down_revision`. If `just db-create-migration` picks something else, trust it and note it.

Body (follow the `78ab0715669f` precedent — `op.execute` for an expression/partial index; `op.create_index` can't express `(metadata->>'dashboard_id')` cleanly):

```python
def upgrade() -> None:
    # One watch_dashboard task per (team, entity, dashboard): the FE watch toggle
    # disables/re-enables a single task per dashboard (FORGE-577), so a second row
    # for the same dashboard is always a bug or a race.
    op.execute("""
        CREATE UNIQUE INDEX uq_scheduled_tasks_team_entity_dashboard
        ON scheduled_tasks (team_id, entity_id, (metadata->>'dashboard_id'))
        WHERE kind = 'watch_dashboard'
    """)


def downgrade() -> None:
    op.drop_index(
        "uq_scheduled_tasks_team_entity_dashboard",
        table_name="scheduled_tasks",
    )
```

Notes:
- **No `refresh_view("scheduled_tasks")`.** Adding an index does not change the table's column list, so the `team_scheduled_tasks` view does not need dropping/recreating. V29 refreshed the view only because it also dropped a column; V32/`78ab0715669f` created partial indexes with no view churn. Do not add view churn here.
- **Not `CONCURRENTLY`.** `CREATE INDEX CONCURRENTLY` cannot run inside a transaction, and the generated Flyway SQL wraps every migration in `BEGIN; … COMMIT;`. Keep it plain, consistent with V29/V32. The `ACCESS EXCLUSIVE` lock is brief on this table.

Then regenerate the paired Flyway SQL:

```bash
just db-generate-migrations
```

⚠️ This script (`apps/api/scripts/generate_migrations.py`) **`rmtree`s and regenerates the whole** `apps/api/platform/defaults/datastores/migrations/` directory from `alembic history`. After running it, `git status` must show **exactly one new file** — `V49__scheduled_tasks_unique_watch_dashboard_per_entity.sql` (name derived from the revision filename) — and **zero modifications to V1…V48**. If V1–V48 changed, something is wrong; do not commit that. Do not hand-write the V49 file.

## 2. Repository: distinguish the two unique violations

`apps/api/src/api/repositories/scheduled_task_repository.py:163-168` currently blanket-maps any `asyncpg.UniqueViolationError` on insert to the title message. Add a module-level constant and branch on `constraint_name` (Postgres reports the index name in the `constraint_name` error field for unique-index violations, and asyncpg exposes it):

```python
_DASHBOARD_UNIQUE_INDEX = "uq_scheduled_tasks_team_entity_dashboard"
```

```python
    except asyncpg.UniqueViolationError as exc:
        if getattr(exc, "constraint_name", None) == _DASHBOARD_UNIQUE_INDEX:
            raise AlreadyExistsException(
                "A dashboard watch task already exists for this dashboard."
            ) from exc
        raise AlreadyExistsException(
            f"A scheduled task titled '{title}' already exists."
        ) from exc
```

- Use `getattr(..., None)` so a violation with no constraint field still degrades to the existing title message — this is also what keeps the current unit test (`test_create_scheduled_task_entry_translates_unique_violation`, which builds `asyncpg.UniqueViolationError("duplicate")` with no fields set) passing unchanged.
- **Do not touch `update_scheduled_task_entry`** (`:249-262`). `entity_id`, `kind` and `metadata` are not updatable columns, so an UPDATE can never violate the new index; its `assert title is not None` stays valid. (Latent fragility, out of scope.)

## 3. Route: no change

`AlreadyExistsException` is already registered globally (`libs/common/src/common/exception_handlers.py:48-55` and `:103`) → **HTTP 409**. Note the handler returns a **generic body `{"detail": "Resource already exists"}`** and discards the exception message — so the integration test must assert only `status_code == 409`, **not** the dashboard-specific string. Do **not** add a route-level `except AlreadyExistsException`; the ticket specifies the global handler.

## 4. Agent tool: map to `ToolError`

`create_scheduled_task` in `apps/api/src/api/agent/agents/skills_agent/tools/scheduled_tasks_tools.py:359` calls the service directly. Unlike the FE, the agent has **no check-then-create guard** (see `skills/dashboard-monitoring.md` step 4 — it just calls the tool), so asking Olly to watch an already-watched dashboard would let `AlreadyExistsException` bubble raw out of the tool and break the conversation. Wrap the service call, mirroring `skill_management_tools.py:213-215`:

```python
    async with ctx.context.get_db_connection() as db:
        try:
            task = await scheduled_tasks_service.create_scheduled_task(
                db, entity_metadata, request
            )
        except AlreadyExistsException as exc:
            raise ToolError(str(exc)) from exc
```

`ToolError` is already imported (`:14`); add `from common.exceptions import AlreadyExistsException`. This also fixes the pre-existing duplicate-title case on the same path. Keep the rest of the `async with` body (the `dashboard_watch_created` stream event) unchanged. Do **not** implement "re-enable the existing task" — that's new product behaviour, not this ticket.

## 5. Tests

### Unit — `apps/api/tests/ut/test_scheduled_tasks_repository.py`
- Add `test_create_scheduled_task_entry_translates_dashboard_unique_violation`: build a `asyncpg.UniqueViolationError("duplicate")`, set `exc.constraint_name = "uq_scheduled_tasks_team_entity_dashboard"`, `mock_db.fetchrow = AsyncMock(side_effect=exc)`, call `create_scheduled_task_entry(... kind=ScheduledTaskKind.WATCH_DASHBOARD, metadata=WatchDashboardMetadata(dashboard_url=DASHBOARD_URL))`, assert `pytest.raises(AlreadyExistsException, match="dashboard")`. If asyncpg refuses the attribute assignment, build the exception via a tiny subclass instead: `type("_DupIdx", (asyncpg.UniqueViolationError,), {"constraint_name": _INDEX_NAME})("duplicate")`.
- Keep `test_create_scheduled_task_entry_translates_unique_violation` (constraint-less → title message) as the regression guard for the fallback branch.
- Existing `test_create_scheduled_task_entry__persists_metadata` (`:257`) already proves `dashboard_id` is persisted into the `metadata` jsonb (it's a pydantic `computed_field`, so `model_dump` includes it) — no change needed, but it's the reason the index expression works.

### Integration — `tests/integration/test_scheduled_tasks_route.py`
Real Postgres with real migrations applied, so the index is genuinely exercised. Add a payload helper:

```python
def _watch_payload(dashboard_id: str, *, title: str) -> dict:
    return _valid_create_payload(
        title=title,
        kind="watch_dashboard",
        metadata={
            "dashboard_url": f"https://app.coralogix.com/#/dashboards/{dashboard_id}"
        },
    )
```
(The route has no `_verify_dashboard_exists` call — that lives only in the agent tool — so no mocking is needed. Existing create tests show `RequireAIConsent` is satisfied by the test fixtures.)

New tests:
1. `test_create_scheduled_task__duplicate_dashboard_conflicts` — POST `_watch_payload("dash-1", title="Watch A")` → 201; POST `_watch_payload("dash-1", title="Watch B")` (**different title**, so it isolates the new index from the title index) → **409**.
2. `test_create_scheduled_task__disabled_watch_still_blocks_duplicate` — create the watch, `PUT /scheduled-tasks/{id}/schedule` with `enabled=false`, then POST the same dashboard again → **409**. Documents the FORGE-577 "disable, don't delete" intent: a disabled task still holds the slot.
3. `test_create_scheduled_task__different_dashboard_allowed` — `dash-1` then `dash-2` → both 201.
4. `test_create_scheduled_task__same_dashboard_other_entity_allowed` — extend `_insert_watch_dashboard_task` (`:132-165`) with an `entity_id: str = USER_ID` keyword arg, seed a row for `entity_id="other-user"` on the same `team_id` + `dash-1`, then POST `_watch_payload("dash-1", ...)` as `USER_ID` → **201**. Proves the constraint isn't team-wide.
5. `test_create_scheduled_task__standard_kind_unaffected` — two `standard` POSTs with different titles and no metadata → both 201.
6. Confirm the existing `test_list_scheduled_tasks_for_dashboard__*` tests still pass unchanged (they insert distinct dashboard/team combinations, so none collide with the new index).

## 6. Ops precondition — run before merging

Against **each deployed environment** (prod + staging), as read-only:

```sql
SELECT team_id, entity_id, metadata->>'dashboard_id' AS dashboard_id,
       count(*) AS dupes, array_agg(id ORDER BY created_at) AS ids
FROM scheduled_tasks
WHERE kind = 'watch_dashboard'
GROUP BY 1, 2, 3
HAVING count(*) > 1;
```

Zero rows → safe to ship. Any rows → **stop**, report in-thread, and treat cleanup as a separate explicit step (out of scope per the ticket). Record the result in the PR description.

## 7. Verification commands (run in the implementation step)

Prereqs: `docker compose up -d` (Postgres/Redis/Restate) and a working `uv sync`.

```bash
# 1. Apply the migration locally; must succeed and create the index
just db-migrate
psql "$DATABASE_URL" -c "\d scheduled_tasks" | grep uq_scheduled_tasks_team_entity_dashboard

# 2. Migration pairing — expect exactly one new V49 file, V1..V48 untouched
just db-generate-migrations && git status --short

# 3. Lint + typecheck (ruff + ty), scoped
just lint-fix && just lint

# 4. Unit tests
just test-api tests/ut/test_scheduled_tasks_repository.py -q

# 5. Integration tests (real Postgres, real migrations)
just test-integration integration/test_scheduled_tasks_route.py
```

**Behaviour to observe, before vs. after** (the ticket's actual definition of done):

- *Before* (stash the migration): POST `/scheduled-tasks/` twice with `kind=watch_dashboard`, the same `dashboard_url`, and different titles → **two 201s**, and `GET /scheduled-tasks/for-dashboard/{dashboard_id}` returns **2 tasks**.
- *After*: same sequence → **201 then 409** (`{"detail": "Resource already exists"}`), and `for-dashboard` returns **1 task**.
- Capture both as CLI/HTTP output into `.saga/artifacts/` (e.g. `api-duplicate-watch-before.txt` / `api-duplicate-watch-after.txt`).

## Risks / edge cases

- **Deploy-blocking migration** if production already holds duplicates — mitigated only by §6. This is the single biggest risk.
- **`watch_dashboard` rows with a NULL/absent `metadata->>'dashboard_id'`** are not constrained (NULLs are distinct in a unique index). Acceptable: `WatchDashboardMetadata._validate_dashboard_id` rejects an unparseable URL at the API layer, so such rows can only be legacy/hand-written. Do not try to backfill.
- **Downgrade** must be verified (`alembic downgrade -1` then `upgrade head`) — `op.drop_index` on an index created via raw SQL works because the name matches.
- **`kind` is a plain `VARCHAR`** (V45), not an enum, so the `WHERE kind = 'watch_dashboard'` literal must exactly match `ScheduledTaskKind.WATCH_DASHBOARD.value`.
- The advisory lock in `_acquire_scheduled_tasks_lock` (`service.py:700-712`) already serialises concurrent creates per `(team_id, entity_id)`, so in practice the index is the correctness backstop rather than the race fix — but it is the only thing that stops a client bug or a non-locking future code path.

## File checklist

Create:
- `apps/api/alembic_migrations/versions/<rev>_scheduled_tasks_unique_watch_dashboard_per_entity.py`
- `apps/api/platform/defaults/datastores/migrations/V49__scheduled_tasks_unique_watch_dashboard_per_entity.sql` (**generated** by `just db-generate-migrations`, not hand-written)

Modify:
- `apps/api/src/api/repositories/scheduled_task_repository.py` (constraint-aware violation mapping)
- `apps/api/src/api/agent/agents/skills_agent/tools/scheduled_tasks_tools.py` (`AlreadyExistsException` → `ToolError`)
- `apps/api/tests/ut/test_scheduled_tasks_repository.py` (new unit test)
- `tests/integration/test_scheduled_tasks_route.py` (new integration tests + `entity_id` kwarg on `_insert_watch_dashboard_task`)

Move/rename/delete: none. Import sites to update: none (no symbols renamed).
