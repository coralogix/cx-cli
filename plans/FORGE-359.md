# FORGE-359 — Add `durationSeconds` to execution events (BE + FE)

## Goal (recap)

- Backend: measure elapsed wall-clock time between START and END for each `REASONING_MESSAGE_*` and `TOOL_CALL_*` block, emit it as `durationSeconds` on `REASONING_MESSAGE_END` / `TOOL_CALL_END` AG-UI events (rounded to 1 decimal), persist per-interaction durations in `responses.execution_durations`, and reproduce the same wire events on historical reload.
- Frontend: render the duration on reasoning rows (`"Thought for 4.2s"`) when present, fallback to `"Thought a bit"` when absent. Tool-call durations are emitted on the wire but not rendered.
- Old data (no `execution_durations`) must reconstruct without error and without a duration label.

## Environment / how to run

Both worktrees are set up but this planner has **not started services** (no local Postgres/Restate/Kafka/dev backend or FE devserver available in this sandbox — bringing them up requires Docker + secrets the sandbox does not have). Commands the implementer will use:

**Backend (`olly/`)**
- `just api::db-create-migration "add_execution_durations_to_responses"` — scaffold Alembic revision file.
- `just api::db-migrate` — apply migrations.
- `just api::test` — run unit tests (`uv run pytest -v tests/`).
- `just api::lint` — ruff + ty + mypy for `src/api/agent`.
- `just api::openapi-generate` — regenerate `apps/api/openapi.json` after the events grow a new optional field. Required so the FE codegen picks the field up.
- Optional full local run: `just dev` (docker-compose + restate + api).

**Frontend (`frontend/`)**
- `pnpm nx run data-olly-api-client:generate-client` — regenerate the OpenAPI client from the updated `apps/api/openapi.json` (see `libs/_data/olly/api-client/openapifetch.toml` — it pulls from `cx-olly` master, so an intermediate manual patch may be needed until the backend PR lands).
- `pnpm nx test olly` — unit tests (includes `chat-messages.utils.spec.ts` and `resolve-execution-rows.spec.ts`).
- `pnpm nx lint olly` — lint.
- Optional full local run: `pnpm run dstaging`.

**Before-state observation (record as an artefact under `.saga/artifacts/`):**
The implementer should, before writing code, load an existing chat in the live UI and capture (screenshot or short screen recording) a reasoning row showing the current `"Thought a bit"` label with no duration. After-state: the same reasoning row showing `"Thought for Xs"`. This is the visible acceptance signal for the FE change.

Ticket text and this planner have already independently verified the current state against the checked-out code — see the ticket's Background section (it re-verifies file paths, deleted-file names, i18n key path, and the `formatExecutionStepDuration` falsy-guard bug), so the plan below is grounded in the current worktree, not the outdated `chat-reasoning.utils.ts` architecture.

## Scope note — FORGE-2 flip-flop

Per the ticket background, FORGE-2 (the original backend split) is still open in Linear (`Backlog`, not `Cancelled`). Before starting backend work, confirm FORGE-2 is either closed as superseded by FORGE-359 or explicitly left inert. PR #452 on `cx-olly` is reference only — do not merge/rebase off it.

## Verified current state (confirmed on the worktree in this planning pass)

- `apps/api/src/api/agent/stream_events.py:262` — `timestamp = _get_start_timestamp_ms_from_response(response)` computed once per response; reused for every event mapped from that response, including `ReasoningMessageEndEvent` (`stream_events.py:394`) and `ToolCallEndEvent` (`stream_events.py:374`). Same coarse timestamp is passed through in the historical path (`_get_events_for_reasoning_output` at `stream_events.py:675`, `_get_events_for_function_call_output` at `stream_events.py:822`).
- `map_sdk_event_to_agui_event` (`stream_events.py:227`) is stateless — no per-stream state today. Confirmed.
- `_emit_agui_events` in `agent_runner.py:412` and `_handle_agent_event_stream` in `agent_runner.py:433-562` are where per-stream state should live. `_handle_response_created` (line 295) calls `finalize_response` for the previous response before creating a new one (line 315); the terminal `finalize_response` is line 557.
- `finalize_response` (`run_agent_utils.py:209-227`) currently writes `tool_responses`, `status`. It's the only site to persist `execution_durations`.
- `responses` table originally created in `apps/api/alembic_migrations/versions/6c8df613ab15_initial_schema.py:220-263`. **Verified: no later migration redefines the `team_responses` view** (`grep -H "team_responses" apps/api/alembic_migrations/versions/*.py` returned no matches), so the `SELECT *` in `TEAM_LEVEL_VIEW_CREATE_TEMPLATE` (same file, lines 21-26) will automatically expose the new column. Re-verify at implementation time in case something lands between now and then.
- `ResponseRecord` (`libs/common/src/common/schemas/chats.py:363-381`) — fields confirmed; needs a new `execution_durations: dict[str, float] | None = None` field. The SQL in `convert_responses_to_agui_events` (`stream_events.py:1255-1272`) needs to add the column to the SELECT.
- ag-ui events: `apps/api/openapi.json` declares `additionalProperties: true` on `ReasoningMessageEndEvent` (`openapi.json:15815`) and the pydantic `BaseEvent` in `ag_ui.core` (via `ag-ui-protocol>=0.1.10` in `pyproject.toml:26`) accepts extra fields — evidenced by `additionalProperties: true`. Passing `durationSeconds=X` as an extra kwarg is expected to serialize verbatim via `model_dump(mode="json", by_alias=True, exclude_none=True)`. **Implementer must confirm at implementation time** by (a) constructing an event in a quick unit test with `durationSeconds` and asserting `model_dump(by_alias=True, exclude_none=True)["durationSeconds"] == expected`, and (b) checking the regenerated `openapi.json` contains the field in the schema (may need an explicit-property patch to the openapi generator if the extra-field emission is not visible in the schema — the emitted JSON payload matters for the wire; the schema entry matters for FE codegen).
- FE current state — confirmed at the paths the ticket cites:
  - `frontend/libs/_data/olly/api-client/src/generated/models/reasoning-message-end-event.ts:9-16` — current shape has no `durationSeconds`; declares `[key: string]: any;` so extra fields at runtime already parse, but the type must be extended for type-safe reads.
  - `frontend/libs/_data/olly/api-client/src/generated/models/tool-call-end-event.ts:9-16` — same shape.
  - `frontend/libs/olly/src/lib/components/chat/messages/chat-messages.utils.ts:578-580` — the `case 'REASONING_MESSAGE_END':` currently only calls `context.runState.completeActiveEvent()`. Confirmed.
  - `frontend/libs/olly/src/lib/components/chat/messages/execution-steps.ts:307` — `completeActiveEvent()` walks back through `displayEvents` and sets `isComplete = true` on the last reasoning row.
  - `frontend/libs/olly/src/lib/components/chat/messages/execution/chat-execution.utils.ts:549-570` — `formatExecutionStepDuration` uses `if (!seconds)` (truthy guard); `durationSeconds: 0` returns null. This is the bug the ticket warns about.
  - `frontend/libs/olly/src/lib/components/chat/messages/execution/resolve-display-event-rows.ts:333` — the reasoning row's `durationLabel` is hard-coded to `null`.
  - i18n keys are at `OLLY.CHAT.EXECUTION.DURATION_SECONDS` / `.DURATION_MINUTES` (`libs/i18n/cx/olly/en.json:159-160`, nested under the `EXECUTION` object opened line 141). No `REASONING` sub-object. Ticket's corrected path is authoritative.

## Plan (backend first, then frontend)

### 1. Backend — Alembic migration

Create `apps/api/alembic_migrations/versions/<new>_add_execution_durations_to_responses.py` via `just api::db-create-migration "add_execution_durations_to_responses"`.

```python
def upgrade() -> None:
    op.add_column(
        "responses",
        sa.Column("execution_durations", JSONB, nullable=True),
    )

def downgrade() -> None:
    op.drop_column("responses", "execution_durations")
```

- Nullable, no default. Pre-migration rows read as `NULL` → replay emits events without `durationSeconds`.
- No view rebuild needed — `team_responses` uses `SELECT *`. Verify with a quick `SELECT execution_durations FROM team_responses LIMIT 0;` after `just api::db-migrate`.
- Schema of the JSONB payload: `{ "<message_id_or_tool_call_id>": <seconds as float rounded to 1 decimal> }`. Reasoning message ids and tool-call ids share the same flat dict — they never collide in practice (openai-generated ids `rs_*` vs `fc_*`/`call_*`).

### 2. Backend — schema & per-stream tracker

**`libs/common/src/common/schemas/chats.py`** — extend `ResponseRecord`:

```python
class ResponseRecord(BaseModel):
    ...
    execution_durations: dict[str, float] | None = None
```

**`apps/api/src/api/agent/stream_events.py`** — introduce a small tracker:

```python
class ExecutionDurationTracker:
    """Records monotonic start times per block/tool-call id and emits deltas at END.

    Scoped to a single OpenAI ``Response`` stream — reset per ``ResponseCreatedEvent``
    in ``_handle_agent_event_stream``. Round to 1 decimal (matches ticket spec).
    Absent id at END => no duration attached; do not fabricate zero.
    """
    def __init__(self) -> None:
        self._starts: dict[str, float] = {}
        self.durations: dict[str, float] = {}

    def start(self, block_id: str) -> None:
        # First writer wins; a duplicate START on the same id is ignored so a
        # noisy stream can't inflate the delta.
        self._starts.setdefault(block_id, time.monotonic())

    def end(self, block_id: str) -> float | None:
        start = self._starts.pop(block_id, None)
        if start is None:
            return None
        delta = max(0.0, round(time.monotonic() - start, 1))
        self.durations[block_id] = delta
        return delta
```

Change `map_sdk_event_to_agui_event(event_data, response, agent_name, *, tracker: ExecutionDurationTracker | None = None) -> list[Event]` and thread it through `_map_sdk_event_to_agui_event_inner`. Inside `_map_sdk_event_to_agui_event_inner`:

- On `ResponseOutputItemAddedEvent` where `item` is `ResponseReasoningItem` (currently emitting `ReasoningMessageStartEvent`): call `tracker.start(item.id)`.
- On `ResponseOutputItemAddedEvent` where `item` is `ResponseFunctionToolCall` (currently emitting `ToolCallStartEvent`): call `tracker.start(tool_call_id)` (resolved as today: `item.id or item.call_id`).
- On `ResponseFunctionCallArgumentsDoneEvent` (`ToolCallEndEvent`): compute `delta = tracker.end(event_data.item_id)`; if not `None`, construct `ToolCallEndEvent(..., durationSeconds=delta)`.
- On `ResponseOutputItemDoneEvent` where item is `ResponseReasoningItem` (`ReasoningMessageEndEvent`): compute `delta = tracker.end(event_data.item.id)`; if not `None`, construct `ReasoningMessageEndEvent(..., durationSeconds=delta)`.

Passing `durationSeconds=` as an extra kwarg relies on ag_ui's `extra="allow"` — see the "Verified current state" note above for the confirmation the implementer must perform before proceeding.

### 3. Backend — wire the tracker through the stream

**`apps/api/src/api/agent/agent_runner.py`**:

- Hold a `current_tracker: ExecutionDurationTracker | None = None` inside `_handle_agent_event_stream` alongside `response`, `tool_responses`, etc.
- In `_handle_response_created`: **before** finalizing the previous response, capture `previous_tracker = current_tracker`; call `finalize_response(db, response, tool_responses, ResponseStatus.COMPLETED, execution_durations=previous_tracker.durations if previous_tracker else None)`. Then assign `current_tracker = ExecutionDurationTracker()`.
- Pass `current_tracker` to `_emit_agui_events` (extra kwarg) and from there to `map_sdk_event_to_agui_event`.
- At the terminal `finalize_response` (line 557 today): pass `execution_durations=current_tracker.durations if current_tracker else None`.
- For stopped / errored paths (`_handle_response_end`, `RunCancelledError`, `ContextWindowExceededError`, `openai.APIError`), ensure whichever `finalize_response` / `update_response_status` runs receives the tracker so partial durations for already-completed blocks persist. Note: `_handle_response_end` marks `response_failed=True` for non-completed events — those flow through the outer `try/except`; the current code does NOT call `finalize_response` on failure paths (it only calls it on success). That means failed-response rows won't have durations; explicit — call this out in the code but do not persist for failed responses in this ticket to keep the change contained. Partial durations for *completed* blocks in a stopped run are preserved because the response completed before cancellation, so the standard finalize path handles it.

### 4. Backend — persistence in `finalize_response`

**`apps/api/src/api/agent/run_agent_utils.py`** — extend:

```python
async def finalize_response(
    db: DBConnection,
    response: Response,
    tool_responses: list[FunctionCallOutput],
    response_status: ResponseStatus,
    execution_durations: dict[str, float] | None = None,
) -> None:
    update_query = """
    UPDATE team_responses
    SET tool_responses = $1::jsonb,
        status = $2,
        execution_durations = COALESCE($3::jsonb, execution_durations)
    WHERE id = $4
    """
    await db.execute(
        update_query,
        tool_responses,
        response_status.value,
        json.dumps(execution_durations) if execution_durations else None,
        response.id,
    )
```

The `COALESCE` preserves any earlier partial write (defensive; there is currently only one write per response). Empty dict → don't overwrite with empty; skip binding by passing `None` when there's nothing to save.

The `run_structured_responses_call` path also calls `finalize_response` (line 589). It has no streamed reasoning/tool events so `execution_durations` stays defaulted to `None` — no changes required at that call site.

### 5. Backend — historical reconstruction

**`apps/api/src/api/agent/stream_events.py`**:

- `convert_responses_to_agui_events` SQL (line 1255): add `execution_durations` to the SELECT column list.
- `_get_events_for_response` (line 977): thread `record.execution_durations` into `_get_events_for_reasoning_output` and `_get_events_for_function_call_output`.
- `_get_events_for_reasoning_output(output_item, response_id, timestamp, execution_durations)`: on the `ReasoningMessageEndEvent` construction (line 675), look up `execution_durations.get(message_id) if execution_durations else None`; if not `None`, pass `durationSeconds=value` as kwarg. Reasoning items with no summary emit no events (short-circuit at line 657) — nothing to do there.
- `_get_events_for_function_call_output(output_item, response_id, timestamp, execution_durations)`: same treatment on the `ToolCallEndEvent` (line 822).
- Empty / missing / null → the field is **absent** from the emitted event (via `exclude_none=True` in the model_dump path that hits the wire), not `null`. This matches the ticket contract.

### 6. Backend — OpenAPI regeneration

Run `just api::openapi-generate`. Confirm `apps/api/openapi.json` picks up `durationSeconds` on `ReasoningMessageEndEvent` and `ToolCallEndEvent`. If the extra-fields emission on ag_ui models doesn't propagate into the schema (a known Pydantic-v2 caveat with `extra="allow"` — schema-time introspection ignores runtime extras), then explicitly patch the openapi export or add a documented Pydantic subclass of the two events (both live in this repo) that redeclares the field. Pick whichever the openapi-generate output requires.

### 7. Backend — tests (`apps/api/tests/ut/test_stream_events.py`)

Add (in the same file, following the existing test-per-function pattern):

- `test_map_sdk_event_to_agui_event__reasoning_end_carries_duration_seconds` — construct a tracker, feed a `ResponseOutputItemAddedEvent` with a reasoning item, sleep a small monotonic delta (`time.monotonic` mocked via `monkeypatch` to return two fixed values), feed the matching `ResponseOutputItemDoneEvent`, assert the emitted `ReasoningMessageEndEvent.model_dump(by_alias=True, exclude_none=True)["durationSeconds"] == 0.1` (or whatever the mocked delta is).
- `test_map_sdk_event_to_agui_event__tool_call_end_carries_duration_seconds` — analogous for `ResponseFunctionCallArgumentsDoneEvent` after a `ResponseOutputItemAddedEvent` with a `ResponseFunctionToolCall`.
- `test_map_sdk_event_to_agui_event__reasoning_end_no_start_omits_duration_seconds` — feed only the END event; assert the emitted event has no `durationSeconds` key (`"durationSeconds" not in dump`).
- `test_map_sdk_event_to_agui_event__zero_duration_still_present` — mock monotonic so `round(delta, 1) == 0.0`; assert `durationSeconds == 0.0` is present (not dropped). This locks in the "not falsy" contract.
- `test_convert_responses_to_agui_events__reasoning_message_with_duration` — extend the existing `_create_reasoning_output` helper (line 585) and DB stub to include `execution_durations={"rs_1": 4.2}`; assert the `REASONING_MESSAGE_END` event dump carries `durationSeconds == 4.2`.
- `test_convert_responses_to_agui_events__reasoning_message_without_duration_column` — `execution_durations = None`; assert historical events emit without the field (regression guard for pre-migration rows).
- Optionally: an integration-level test through `_handle_agent_event_stream` that walks the tracker → `finalize_response` → back through `convert_responses_to_agui_events` roundtrip. Only add if the existing test scaffolding makes this easy; otherwise defer.

### 8. Frontend — type patches

**`frontend/libs/_data/olly/api-client/src/generated/models/reasoning-message-end-event.ts`** and **`tool-call-end-event.ts`**:

```ts
export interface ReasoningMessageEndEvent {
  messageId: string;
  rawEvent?: null;
  timestamp?: (number | null);
  type?: 'REASONING_MESSAGE_END';
  durationSeconds?: number;  // NEW — see FORGE-359

  [key: string]: any;
}
```

- Prefer regenerating via `pnpm nx run data-olly-api-client:generate-client` after the backend PR merges to `cx-olly` master (that's where `openapifetch.toml` points). Until then patch the two files manually with a comment referencing the ticket so the next codegen produces a diff-free result once the openapi.json ships.
- Field is `number | undefined`, **never** `number | null`. The wire format has the key absent, not `null`, per the ticket.

### 9. Frontend — capture and thread the field

**`frontend/libs/olly/src/lib/components/chat/messages/execution/chat-execution.types.ts`** — extend `ExecutionDisplayEvent`:

```ts
export interface ExecutionDisplayEvent {
  ...
  durationSeconds?: number; // present only after REASONING_MESSAGE_END (or historical replay)
}
```

**`frontend/libs/olly/src/lib/components/chat/messages/execution-steps.ts`** — extend `completeActiveEvent` to accept and store the duration:

```ts
completeActiveEvent(durationSeconds?: number): void {
  if (!this.activeStep) return;
  for (let index = this.displayEvents.length - 1; index >= 0; index -= 1) {
    const event = this.displayEvents[index];
    if (event.parentStepId !== this.activeStep.id) continue;
    if (event.kind !== 'reasoning') continue;
    if (!event.isComplete) event.isComplete = true;
    if (durationSeconds !== undefined) event.durationSeconds = durationSeconds;
    return;
  }
}
```

Note the `!== undefined` guard (matches the ticket's "not falsy" contract).

**`frontend/libs/olly/src/lib/components/chat/messages/chat-messages.utils.ts`** — extend the REASONING_MESSAGE_END case (line 578-580):

```ts
case 'REASONING_MESSAGE_END': {
  const raw = data['durationSeconds'];
  const durationSeconds = typeof raw === 'number' ? raw : undefined;
  context.runState.completeActiveEvent(durationSeconds);
  break;
}
```

Deliberately not consulting `messageId` — `completeActiveEvent` already targets the most recent open reasoning row for the active step, which is the one the END corresponds to. The event's `messageId` is only useful as a defensive tiebreaker in a multi-reasoning-per-step scenario; keep the existing behaviour to minimise churn. Add a code comment linking to this decision.

### 10. Frontend — render the label

**`frontend/libs/olly/src/lib/components/chat/messages/execution/chat-execution.utils.ts`** — add a dedicated formatter (do NOT reuse `formatExecutionStepDuration`, which is buggy for zero and is used by tool-call rows we don't touch):

```ts
export function formatReasoningRowDuration(
  seconds: number | undefined,
): VisibleToolCall['durationLabel'] {
  if (seconds === undefined) {
    return null;
  }
  // Reuses OLLY.CHAT.EXECUTION.DURATION_SECONDS / DURATION_MINUTES; branching identical
  // to formatExecutionStepDuration except for the guard.
  if (seconds < 60) {
    return { key: 'OLLY.CHAT.EXECUTION.DURATION_SECONDS', params: { seconds } };
  }
  const minutes = seconds / 60;
  const rounded = Math.round(minutes * 10) / 10;
  const display = Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
  return { key: 'OLLY.CHAT.EXECUTION.DURATION_MINUTES', params: { minutes: display } };
}
```

**`frontend/libs/olly/src/lib/components/chat/messages/execution/resolve-display-event-rows.ts`** — in `resolveExecutionRowsFromDisplayEvents` (line 322 area), replace `durationLabel: null,` for reasoning rows with:

```ts
durationLabel:
  event.kind === 'reasoning'
    ? formatReasoningRowDuration(event.durationSeconds)
    : null,
```

The template at `row/chat-execution-row.component.html:29-33` already renders `durationLabel` when present — no template change needed. The `THOUGHT_A_BIT` completed-panel header fallback (in `resolveCompletedPanelHeaderKey`, line 512-528) stays as-is; it's the panel-level label, not the per-row label the ticket is about.

### 11. Frontend — tests

- **`frontend/libs/olly/src/lib/components/chat/messages/chat-messages.utils.spec.ts`** — add a test that feeds a `REASONING_MESSAGE_END` event with `durationSeconds` and asserts the stored `displayEvent.durationSeconds` matches. Add a companion test with `durationSeconds: 0` and confirm the row's `durationLabel` still renders (regression guard against the falsy-guard bug the ticket flags).
- **`frontend/libs/olly/src/lib/components/chat/messages/execution/resolve-execution-rows.spec.ts`** — extend the existing "reasoning row" scenario at lines 1908 / 1953 (both currently assert `durationLabel` is `null`) to a new sibling test that provides `durationSeconds` and asserts the label formats to `"OLLY.CHAT.EXECUTION.DURATION_SECONDS"` with `{ seconds: X }`. Do NOT change the existing assertions — they should still hold when `durationSeconds` is absent (historical fallback).
- **`frontend/libs/olly/src/lib/components/chat/messages/execution/chat-execution.utils.spec.ts`** — add a small direct test of the new `formatReasoningRowDuration` covering `undefined`, `0`, `4.2`, `65`, `600`.

## Order of changes (dependency-first)

1. Backend migration → `just api::db-migrate`.
2. `ResponseRecord` + tracker + `map_sdk_event_to_agui_event` signature change (unwired) — code compiles.
3. Wire tracker into `_handle_agent_event_stream` + `finalize_response`.
4. Extend historical reconstruction (SELECT + reader → END events).
5. Backend tests. Run `just api::test` + `just api::lint`.
6. `just api::openapi-generate` → verify schema.
7. Regenerate FE client (or manual patch) with the new field.
8. FE type + capture + formatter + row wiring.
9. FE tests. Run `pnpm nx test olly` + `pnpm nx lint olly`.
10. Manual before/after screenshot / recording capture into `.saga/artifacts/`.

Every step should keep the tree in a "runnable" state (types compile after step 2 because the kwarg is optional; historical events still work after step 4 because `execution_durations` defaults to `None`).

## Edge cases / risks

- **Absent vs null**: on the wire the field is **absent** when unknown, not `null`. Uses `exclude_none=True` in `serialize_response_for_storage`-style dumps (verified to already be the mode in `emit_stream_events` at `stream_events.py:566` — `model_dump(mode="json", by_alias=True)` there doesn't have `exclude_none=True`, but `ag_ui` sets `.durationSeconds=None` only if we set it; since we conditionally set it only when the tracker returns a delta, missing → attribute never assigned → dump omits it iff `extra="allow"` treats unassigned extras as absent. **Verify at implementation time** with a `model_dump(mode="json", by_alias=True)` on an event where `durationSeconds` was never passed — the key must not appear.
- **Zero-duration**: `round(0.02, 1) == 0.0`. The tracker still records it; the FE formatter's `seconds === undefined` guard renders `"0s"`. Locked in by tests on both sides.
- **Ordering**: `time.monotonic()` — monotonic, immune to wall-clock adjustments; safe for cross-response arithmetic *within a single process*. We never subtract across processes, so this is correct.
- **Duplicate START on same id**: tracker uses `setdefault` — first START wins; a re-added item cannot inflate the delta.
- **Multi-response streams**: the tracker is reset on each `ResponseCreatedEvent`; each response persists its own durations to its own row. Sub-agent durations therefore live on each sub-agent's `responses` row automatically.
- **Sub-agent `run_structured_responses_call` / compaction**: no streamed reasoning/tool events → `execution_durations` stays `None`. No plumbing needed.
- **Cancellation / errors**: `_handle_agent_event_stream` catches `RunCancelledError` and returns without calling the terminal `finalize_response`. That means durations for a cancelled response are not persisted. If the previous response in the same stream already completed, its durations were persisted at the `_handle_response_created` boundary. Ticket accepts this: "stopped/error interactions must persist partial durations for completed blocks" — which for us means "for completed *responses*, not for the response that was mid-flight when cancelled". Call this out in a code comment; add an explicit test that a cancelled-mid-response scenario doesn't crash.
- **Pre-migration rows**: `execution_durations IS NULL` → `record.execution_durations` is `None` in `ResponseRecord` → helpers skip attaching `durationSeconds` → FE gets events with the field absent → `durationLabel` is `null` → row falls back to no-duration display. Explicit test on both sides.
- **`team_responses` view**: relies on `SELECT *` — re-verify no migration between now and merge redefined the view (rerun `grep team_responses apps/api/alembic_migrations/versions/*.py`).
- **ag_ui `extra="allow"` schema visibility**: contingency in step 6 above — if OpenAPI export doesn't reflect the extra field, add a thin project-local subclass of the two events that declares `durationSeconds: float | None = None` and switch the call sites to it; behaviour on the wire is unchanged.
- **`chat-execution.utils.ts:1725` `durationSecondsFromLabel`**: this is the panel-total summation for merged tool-call rows. Reasoning rows never merge with tool-call rows (different `event.kind`), so this path is unaffected by the reasoning-duration change. Confirmed by reading `canMergeSteps` (line 574) and `mergeSteps` (line 596).
- **Frontend codegen drift**: if the manual patch is applied but the backend PR hasn't merged to `cx-olly` master, the next `pnpm nx run data-olly-api-client:generate-client` will overwrite the patch. Guarded by making the FE PR depend on the backend PR merging first (open the FE PR against a backend commit that has the openapi diff, and merge backend before running FE codegen).

## Verification checklist (before declaring done)

- [ ] `just api::db-migrate` upgrades and downgrades cleanly on a scratch DB (spot-check with `SELECT execution_durations FROM team_responses LIMIT 0;` before/after).
- [ ] `just api::test` — new unit tests green; existing `test_convert_responses_to_agui_events__reasoning_message` still green.
- [ ] `just api::lint` — clean.
- [ ] `just api::openapi-generate` — resulting `apps/api/openapi.json` diff includes `durationSeconds` on both event schemas.
- [ ] `pnpm nx test olly` — new + existing tests green.
- [ ] `pnpm nx lint olly` — clean.
- [ ] Manual: load an old chat (pre-migration) → reasoning rows render without duration, no console errors.
- [ ] Manual: run a fresh chat → reasoning row shows `"Xs"` (or `"Y.Zm"`) matching approx wall-clock.
- [ ] Manual: reload the fresh chat → same duration persists (proves persistence + historical path).
- [ ] Screenshots / short recording of the before/after row appearance captured under `.saga/artifacts/`.
