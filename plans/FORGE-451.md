# FORGE-451 — Olly timeout should not discard current turn

## Goal
When Olly hits `AGENT_INTERACTION_TIMEOUT_SECONDS` (15 min), stop discarding the turn. Preserve the partial assistant work (reasoning, messages, tool calls, tool outputs) in `team_responses`, surface a distinct "would you like to continue?" state to the frontend, and offer a **continue** action that resumes the same interaction with the preserved work as input to the next agent run — instead of today's error banner + retry-that-re-sends-the-original-message flow.

The write-side pattern is the same one `_cleanup_stopped_interaction` already uses for user-initiated stop (`agent_interaction.py:606-696`). The replay-side mechanism is the same one `get_input_from_previous_responses` already uses (`response.py:345-430`). We introduce a new terminal status `TIMED_OUT` (parallel to `STOPPED`) so the two flows are distinguishable end-to-end, and a new `POST .../interactions/{id}/continue` endpoint so the frontend can trigger resumption without a new user message.

Out of scope: the timeout value itself, the broader errors/notifications redesign, and any cap on repeated re-continues.

---

## How to run / verify

- **Repo layout**: `olly/` is a Python monorepo managed with `just` + `uv`; `frontend/` is the Angular Nx monorepo.
- **Backend dev**: `just dev` (`olly/`) — runs migrations then FastAPI + Restate; requires Docker services (Postgres, Redis, Restate, LiteLLM). See `/run-dev-env` skill.
- **Backend checks**: `/code-checks` (project skill) runs lint + UT + integration tests in parallel. Individually:
  - Lint: `just lint` (olly root)
  - Unit tests: `just test-api`
  - Integration tests: `just test-integration` (loads `.env.test`)
  - Common lib tests: `just test-common`
- **Frontend checks**: `pnpm nx lint olly` (or affected), `pnpm nx test olly` (or affected).
- **Observation of current behavior**: full end-to-end reproduction requires the running stack (Postgres + Restate) and 15 min of wall time. Use the existing integration test at `olly/tests/integration/test_agent_interaction_errors.py::test_interaction_timeout_persists_timeout_error_code` as the reproduction — with `AGENT_INTERACTION_TIMEOUT_SECONDS` monkey-patched to 1s and a mocked slow `Runner.run_streamed`, it observes the current behavior: after the timeout, `team_responses` is deleted and a single pseudo-response with `status='error'`, `error_code='TIMEOUT'` remains. That row is the "before-state" the plan modifies.

Record before/after artifacts by capturing the DB rows for a timed-out interaction from that test (a CLI output artifact is sufficient — no browser reproduction is required).

---

## What needs to change and why

### Backend (Python, `apps/api`, `libs/common`)

#### 1. New terminal state `TIMED_OUT` on both response and interaction

**File:** `libs/common/src/common/schemas/chats.py`
- Add `TIMED_OUT = "timed_out"` to `ResponseStatus`. Place it next to `STOPPED` with a comment: "The interaction hit AGENT_INTERACTION_TIMEOUT_SECONDS; partial work is preserved and can be resumed via `POST .../interactions/{id}/continue`."

**File:** `libs/common/src/common/schemas/interactions_schema.py`
- Add `TIMED_OUT = "timed_out"` to `InteractionStatus` with a matching docstring line.

**Why not reuse `STOPPED`:** `STOPPED` is user-initiated stop (the user pressed the stop button, intent is "I'm done for now"). `TIMED_OUT` is server-initiated preservation (intent is "I ran out of time, ask the user whether to continue"). The frontend must render a different UI (continue banner vs. quiet stop), and the interaction status must be distinguishable in `get_interaction` / list responses. Rolling both into one enum value forces `error_code` sniffing on every consumer.

**Why not `ERROR` + `error_code=TIMEOUT`:** the ticket explicitly requires `is_continueable_response()` to be true for the resulting state (`response.py:258-314`). ERROR short-circuits to `False` there. Changing that behavior for one error code creates cross-cutting rules; introducing a new status is cleaner.

#### 2. Treat `TIMED_OUT` as continueable and replay it in full

**File:** `apps/api/src/api/agent/response.py`

- `is_continueable_response` (line 258): add `ResponseStatus.TIMED_OUT` to the passthrough list next to `STOPPED`, so `read_clean_responses` (line 556) keeps timed-out responses as valid history prefix.
- `get_input_from_previous_responses` (line 345):
  - In the status-gate at line 388 (`response.status in [COMPLETED, RUN_COMPLETED, STOPPED]`), add `TIMED_OUT`.
  - Do **not** apply the "STOPPED → strip non-text output" filter (line 394) to `TIMED_OUT`. The ticket requires reasoning/messages/tool-call output to replay as input to the next run.
  - In the tool_responses inclusion gate at line 416 (`if response.status is not ResponseStatus.STOPPED`), leave `TIMED_OUT` on the "include tool_responses" side so any completed tool round-trips are replayed.
- Do **not** change compaction / prefix logic — TIMED_OUT is only a new terminal marker on top of existing plumbing.

**Edge case — dangling `function_call` without matching `function_call_output`:** if the run timed out mid-tool-call, the response would contain a `function_call` output item with no matching entry in `tool_responses`. Replaying that as-is to the LLM breaks the conversation invariant (OpenAI/Anthropic reject unmatched tool calls). Handle in the roll-up step (§3) by mirroring the STOPPED path's `is_continueable_response()` mismatch check: if the response has a `function_call` without a matching `tool_response`, drop that particular output item from the persisted `response.output` during finalize — same net effect as the STOPPED "text-only" fallback, but per-item instead of blanket text-only. This keeps reasoning + text + completed tool round-trips, and only drops the truly unrecoverable dangling call.

#### 3. Timeout cleanup — parameterize the existing stopped-cleanup path

**File:** `apps/api/src/api/agent/handlers/agent_interaction.py`

Refactor `_cleanup_stopped_interaction` (line 606) to accept a `finalize_status: ResponseStatus` and an optional `error_code: str | None`. The existing STOPPED behavior stays intact:
- User-stop → call with `finalize_status=STOPPED`, `error_code=None`. For continueable responses the loop still picks `RUN_COMPLETED` as the roll-up (preserving today's semantics that a fully-completed round survived even if the user cancelled after).
- Timeout → call with `finalize_status=TIMED_OUT`, `error_code=RunErrorCode.TIMEOUT.value`. For **timeout**, override the "continueable → RUN_COMPLETED" branch: use `TIMED_OUT` unconditionally for the in-progress and stream-completed responses, because the interaction as a whole is timed out even if one sub-response happened to finish cleanly. This is what makes the derived `InteractionStatus` come out as `timed_out` (§5) and keeps the "continue?" prompt visible to the user.

While in the response-finalization loop, when finalizing a response whose output has `function_call` items without matching `tool_responses`, strip those unmatched function_call items from `response.output` before `rollup_tokens_to_response`. This handles the edge case in §2 at write time.

Pass the `error_code` through to `_insert_pseudo_response` so a pseudo-response created for the "no responses survived cleanup" branch (line 693) carries `TIMEOUT` as its error_code.

Expose the timeout entrypoint as `async def _cleanup_timed_out_interaction(...)` — a thin wrapper around the refactored core — mirroring the naming of `_cleanup_stopped_interaction`. This keeps the Restate `ctx.run(...)` call site self-documenting.

Add a `logger.warning("Cleaning up timed-out interaction")` at the start of the wrapper.

#### 4. Route `TerminalError(TIMEOUT)` to the timeout-cleanup instead of `_cleanup_failed_interaction`

**File:** `apps/api/src/api/agent/handlers/agent_interaction.py`, `handle_user_message` (line 750, esp. the `except TerminalError` block at 801-848)

The `TerminalError` message is one of:
- `INTERACTION_CANCELLED_MESSAGE` → today: `_cleanup_stopped_interaction`
- `RunErrorCode.TIMEOUT.value` → today: falls through to `_cleanup_failed_interaction`
- anything else → `_cleanup_failed_interaction`

Split the TIMEOUT case out:

```python
if e.message == INTERACTION_CANCELLED_MESSAGE:
    ... (unchanged)
elif e.message == RunErrorCode.TIMEOUT.value:
    await INTERACTIONS_TIMEOUT_COUNT.restate_add(ctx, 1, {...})  # dedup with inner TimeoutError metric
    await ctx.run(
        "cleanup_timed_out_interaction",
        _cleanup_timed_out_interaction,
        args=(run_context, request.user_message),
    )
else:
    ... (existing FAILED_INTERACTIONS_COUNT + _cleanup_failed_interaction)
```

Metrics note: `INTERACTIONS_TIMEOUT_COUNT.add(...)` is currently emitted inside `_handle_agent_interaction`'s `except TimeoutError` block (line 462) — only for the built-in `TimeoutError`. Wrapped LLM/HTTP timeouts (`openai.APITimeoutError`, `httpx.TimeoutException`) raise `TerminalError(TIMEOUT)` from within the inner exception handler at line 442-443, bypassing that metric. Moving the metric to the outer TerminalError branch (or emitting it in both places and de-duplicating with a boolean) fixes that undercount alongside this change; either is fine — the requirement is that every `TIMEOUT` outcome increments it exactly once. Recommend: leave the inner `TimeoutError` block as-is but skip its `INTERACTIONS_TIMEOUT_COUNT.add` there and rely on the new outer branch — simplest single-source-of-truth.

Do **not** call `_notify_scheduled_task_completion_if_needed` differently — the existing call at the end of the TerminalError block still fires with `is_interaction_cancelled=False`, which is correct: a timeout is not a user-initiated cancel.

#### 5. Interaction status derivation — add `timed_out` to the SQL rollup

**File:** `apps/api/src/api/repositories/interactions_repository.py` (`_INTERACTION_STATUS_SQL`, line 30, and `get_interaction_status`, line 141)

Update to:

```sql
CASE
    WHEN bool_or(status = 'error')         THEN 'error'
    WHEN bool_or(status = 'stopped')       THEN 'stopped'
    WHEN bool_or(status = 'run_completed') THEN 'completed'
    WHEN bool_or(status = 'timed_out')     THEN 'timed_out'
    ELSE 'in_progress'
END
```

**Precedence rationale:** on a **successful continue**, the new final response is `RUN_COMPLETED` while the earlier top-level responses remain `TIMED_OUT`. Because `run_completed` sits above `timed_out` in the CASE, the interaction correctly resolves to `completed`. On a **repeated timeout** during continue, only `timed_out` rows exist → interaction is `timed_out` (as required). `error` / `stopped` retain today's precedence.

No Alembic migration is needed — `team_responses.status` is stored as text and the check constraint (if any) needs to be updated to include the new value; verify in `alembic/versions/` whether a check constraint exists on this column. If it does, add an Alembic migration under `apps/api/alembic/versions/` following the `.claude/skills/database-migrations` workflow (`just db-create-migration`) to alter the constraint. Same check for `team_interactions` derived status — that column is *derived*, not stored, so nothing to migrate there.

#### 6. `_get_events_for_response` — emit `RunFinishedEvent` for `TIMED_OUT`

**File:** `apps/api/src/api/agent/stream_events.py` (`_get_events_for_response`, line 1101 area)

Currently:
- `record.status == ERROR` → emit `RunErrorEvent` with the mapped error code.
- `record.status in [RUN_COMPLETED, STOPPED]` → emit `RunFinishedEvent`.

Add `TIMED_OUT` to the `RunFinishedEvent` branch — the run really did finish (from the FE's stream-completion perspective); the "continue" prompt is driven off `InteractionStatus`, not from a stream error. This matches how `STOPPED` is handled today (no `RunErrorEvent` on replay; the stopped status is observed via `InteractionStatus`).

`convert_responses_to_agui_events` then correctly replays the timed-out interaction as a completed stream from the client's perspective.

#### 7. `POST /v2/chats/{chat_id}/interactions/{interaction_id}/continue`

**Files:**
- `apps/api/src/api/routes/v2/interactions_route.py` — new route.
- `apps/api/src/api/services/interactions_service.py` — new service function.
- `libs/common/src/common/schemas/interactions_schema.py` — new `ContinueInteractionRequest` (empty body or a minimal `{"should_block": bool, "timeout_seconds": int}`).

**Route:**

```python
@router.post("/{interaction_id}/continue")
async def continue_interaction(
    chat_id: uuid.UUID,
    interaction_id: uuid.UUID,
    request: ContinueInteractionRequest,
    team_id: TeamIdDep,
    entity_metadata: EntityMetadataDep,
    auth: CoralogixAuthDep,
    db: DBConnection,
    interaction_source: InteractionSourceDep,
) -> InteractionReadAdvanced:
    """Resume a timed-out interaction. No new user message; the agent picks up
    from the preserved partial work (reasoning / messages / tool round-trips)."""
```

**Service — `continue_interaction`:**

Preconditions (fail 400/404/409 as appropriate):
- Interaction exists, belongs to `entity_id`, and is in `InteractionStatus.TIMED_OUT`. Any other status → 409 with a message.
- Standard access gate: `verify_interaction_allowed(...)` (consent + payments) — reuse for consistency with dispatch/update.

Then reuse the same Restate object handler with a **synthetic empty user message** referencing the same `interaction_id`:

- Build `SendMessageRequest` with `content=[]` and no attachments / skills / files. This is the "resume" sentinel.
- Preserve `model_choice` / `data_sources` / `reasoning_effort` from the existing `team_interactions` row (fetch via `interactions_repository.get_interaction_metadata`) so the agent runs with the same settings the timed-out turn used.
- Do **not** call `interactions_repository.create_interaction` — the interaction row already exists.
- Do **not** call `interactions_repository.stop_interaction` — the timed-out interaction is no longer running.
- Call `RestateClient.call_object("AgentInteractions", "handleUserMessage", object_id=str(chat_id), parameters=HandleUserMessageRequest(..., interaction_id=<same>, user_message=<empty>, ...))`.

The `handle_user_message` Restate entrypoint keys off `chat_id`; Restate serializes concurrent handler calls on the same chat, so no lock is needed. Reusing `handleUserMessage` avoids inventing a second entrypoint and gets the existing cancellation + error-cleanup plumbing for free.

**Agent-side "resume mode" hook:**

In `_handle_agent_interaction` / `_load_state` / `load_agent_run_input`, "same interaction_id as last response" already means "no new user_message added" (`load_agent_run_input`, line 769). That's exactly what we want. But we need to guard against a corner case: `_build_user_message_input` at line 776 is called `if is_new_interaction` — for continue, this is False, so no synthetic user message is added. Good.

However, `dispatch_interaction` today always generates a fresh `interaction_id` and inserts the interaction row before calling Restate. `continue_interaction` must skip both, and pass the existing `interaction_id` through. Do this by not going through `dispatch_interaction` — write a small parallel path in the service that:
1. Verifies access.
2. Constructs `HandleUserMessageRequest` directly with the existing `interaction_id`, empty message, and the existing model/data_sources/reasoning.
3. Calls Restate.
4. Blocks/returns like `dispatch_interaction` does.

Add a docstring on the new service explaining that this deliberately does **not** create an interaction row nor call `stop_interaction`, and why (contrast with `update_interaction`).

**Guard rail — empty content should not accidentally become a user-facing message:**
- In `_build_user_message_input` (`response.py:329`), an empty `content=[]` produces a `Message(role="user", content=[])`. Since we only take this path when `is_new_interaction=True`, and continue enters with `is_new_interaction=False` (last response has same interaction_id), no empty user message is emitted. Add an assertion at the top of `_handle_agent_interaction` (or in `continue_interaction` before calling Restate): if `user_message.content == []`, then the last top-level response must exist and match `run_context.interaction_id`; otherwise raise a clean `TerminalError("INVALID_CONTINUE_STATE")`. This prevents accidental empty-message dispatches from other code paths.

**Blocking / streaming semantics:** identical to `dispatch_interaction`. The frontend uses `should_block=false` today and reads the AG-UI event stream; that continues to work because `emit_stream_events` is called during the agent run.

### Frontend (Angular, `frontend/libs/olly`)

#### 8. Distinguish `timed_out` from `error` in the FE model

**Files:**
- `frontend/libs/olly/src/lib/components/chat/messages/failed-interaction-banner/chat-failed-interaction-banner.config.ts` — remove the `TIMEOUT` entry from `ERROR_CONFIG` (the "Olly took too long to respond, please try again" retry banner). TIMEOUT no longer flows through this banner.
- Same folder, `chat-failed-interaction-banner.types.ts` — drop the `RUN_ERROR_TIMEOUT` / `RUN_ERROR_TIMEOUT_DETAIL` keys **only if they're not reused** elsewhere; grep first (already checked — used only in the banner).

Create a new component (or new mode in the existing banner) — recommend a **new component** `cx-olly-chat-timed-out-continue-banner` for clarity, in the same folder pattern. It renders:
- "Olly hit the time limit. Would you like to continue?" (add new i18n keys `OLLY.CHAT.TIMED_OUT_CONTINUE_TITLE` and `OLLY.CHAT.TIMED_OUT_CONTINUE_DETAIL`).
- A single `Continue` button.
- Emits a `continue` output with `interactionId: string`.

**File:** `frontend/libs/olly/src/lib/components/chat/messages/chat-messages.component.html`

Where the existing `<cx-olly-chat-failed-interaction-banner>` is rendered (lines 32-43 and 82-89), add a sibling `@if` branch for `TIMED_OUT` interactions that renders `<cx-olly-chat-timed-out-continue-banner>` instead. Drive that off an `InteractionStatus` map (the frontend already has this via `interactionStatuses()` or similar). If the frontend today only knows failed vs. non-failed, extend the parent (`chat-messages.component.ts` at line 414-428 area, `errorCodeForInteractionId` and neighbors) to expose a `timedOutInteractionIds` signal built from `InteractionMetadataRead.status`.

**File:** `frontend/libs/olly/src/lib/components/chat/chat.component.ts`

Add `protected onContinue(interactionId: string): void { ... }` next to `onRetry` (line 1484). It calls a new `v2InteractionsContinueInteraction` operation on the generated `V2InteractionsService` client. On success, reload the message stream (the existing Electric SQL stream will pick up new responses live). Unlike `onRetry` it does not build a pending user-message bubble, doesn't call `#retryingInteractionId.set(...)`, and doesn't touch `#pendingMessages`.

Regenerate the FE API client after §7 adds the new endpoint (this repo generates TS clients from the OpenAPI spec — invoke whatever the standard generator entrypoint is; grep for how the current `V2InteractionsService` is produced).

Add a component test at the `frontend/libs/olly/**` component-test tier for the new banner (per `frontend/CLAUDE.md`'s Vitest component-test convention).

### Tests

**Extend** `olly/tests/integration/test_agent_interaction_errors.py`:

- `test_interaction_timeout_persists_timeout_error_code` — assert **new** behavior:
  - No `DELETE FROM team_responses` happens on timeout — any partial rows survive.
  - The top-level response for the interaction has `status='timed_out'` (not `'error'`).
  - `error_code='TIMEOUT'` is preserved on that row.
  - `get_interaction_status(interaction_id)` returns `InteractionStatus.TIMED_OUT`.
- Extend `test_wrapped_timeout_persists_timeout_error_code` in the same way for `openai.APITimeoutError`, `httpx.ReadTimeout`, and the wrapped-cause variants.
- **New test** `test_timed_out_interaction_is_continueable`:
  - Simulate a timed-out interaction with at least one prior response containing text output (patch `Runner.run_streamed` to first produce a partial response then time out).
  - Call the new `POST .../interactions/{interaction_id}/continue` endpoint.
  - Assert `handleUserMessage` was invoked with the **same** `interaction_id` and an empty user message content.
  - Assert `read_clean_responses` returned the timed-out response (it was not deleted) and `is_continueable_response(...)` is `True` for it.
- **New test** `test_continue_on_non_timed_out_interaction_returns_409`: sanity guard.
- **New test** `test_get_input_from_previous_responses_replays_timed_out_reasoning`:
  - Build a `ChatResponseRead` with `status=TIMED_OUT`, an output containing `reasoning`, `message`, and one `function_call` **with** a matching `function_call_output`.
  - Call `get_input_from_previous_responses` and assert all three item types round-trip into the returned input (reasoning is not stripped, function call replays with its output).
- **New test** `test_dangling_function_call_dropped_on_timeout_finalize`: verifies §2/§3 edge case — a `function_call` without matching `tool_response` is stripped from the persisted response so replay doesn't send unmatched calls to the LLM.

Existing tests: `test_gateway_quota_block_returns_403_not_500` — unrelated, must continue to pass.

---

## Order of changes (dependencies first)

1. Add `ResponseStatus.TIMED_OUT` and `InteractionStatus.TIMED_OUT` (§1). Trivial, unlocks the rest.
2. Update `is_continueable_response` + `get_input_from_previous_responses` (§2).
3. Update `_INTERACTION_STATUS_SQL` + `get_interaction_status` (§5). Add Alembic migration for the `status` check constraint on `team_responses` if one exists.
4. Refactor `_cleanup_stopped_interaction` to accept `finalize_status` + wrap as `_cleanup_timed_out_interaction` (§3).
5. Route the `TerminalError(TIMEOUT)` branch in `handle_user_message` (§4).
6. Extend `_get_events_for_response` for `TIMED_OUT` (§6).
7. New continue endpoint + service function (§7). Regenerate the FE API client.
8. Frontend: expose `timed_out` interaction status, add continue banner, add `onContinue` handler (§8).
9. Extend integration tests + add new ones. Run `/code-checks`.

Steps 1-6 keep the backend consistent even before the frontend ships: the interaction just parks in `timed_out` state visible via `GET /v2/chats/{chat_id}/interactions/{id}`. Adding the continue endpoint (7) doesn't break existing clients. The FE change (8) can ship in the same PR or one PR behind.

---

## Edge cases and risks

- **Repeated continues that themselves time out**: allowed by the ticket ("no special-cased limit unless a human decides otherwise"). Each continue is a fresh run with the same interaction_id; a subsequent timeout re-enters `_cleanup_timed_out_interaction` and finalizes the newly added in-progress responses to `TIMED_OUT`. Aggregate status remains `timed_out` because no `run_completed` row exists yet. `is_top_level=TRUE` rows keep accumulating across continues on the same interaction — that's fine and matches how the interaction row is shared.
- **Continue against an interaction that has since been edited/replaced**: `update_interaction` (`interactions_service.py:373`) deletes the interaction row entirely. The continue endpoint must return 404 for a missing interaction, which the `interactions_repository.get_interaction_metadata` fetch handles naturally (raises `NotFoundException` → 404).
- **User sends a new message instead of continuing**: works today — a new message creates a new interaction; the timed-out prior responses are already continueable (§2) so they become context to the new interaction via `read_clean_responses`, and only their text is replayed if their status is TIMED_OUT. Wait — TIMED_OUT will now replay *full* outputs (§2). That's the intended behavior even in the "new message not continue" path: the timed-out reasoning/tool outputs become high-quality context for whatever the user asks next. If the reporter wants text-only replay in that case (like STOPPED), we'd need to differentiate "continue mode replay" from "new message replay." Ask the assignee if they want that; the plan chooses full replay everywhere because the ticket asks for maximum preservation.
- **Dangling `function_call` at timeout**: covered in §2/§3 — strip unmatched function_call items at finalize time so replay doesn't send unmatched calls.
- **`_handle_agent_interaction`'s outer `except TimeoutError` (line 462) emits a `RunErrorCode.TIMEOUT` stream event**: harmless — the stream events are ephemeral (`delete_stream_events` is invoked in cleanup). The FE observes the final state through the persisted responses + interaction status, not the transient error event.
- **`asyncio.timeout` may fire during compaction / stream-emission, not inside the agent run**: `_load_state` may not have produced any response yet, so `_cleanup_timed_out_interaction` will find `responses=[]` and fall through to the "no responses survived cleanup" branch — inserting a pseudo-response with `status=TIMED_OUT`, `error_code=TIMEOUT`. That's still continueable (§2), so the continue endpoint would rerun from scratch — effectively a no-op replay because there's nothing to resume from. Acceptable; not worse than today's blank error.
- **`INTERACTIONS_TIMEOUT_COUNT` double-count**: handled explicitly in §4.
- **`_notify_scheduled_task_completion_if_needed`**: fires with `is_interaction_cancelled=False` for the new branch, so a scheduled task run whose interaction times out is reported as "finished" (not "cancelled"), just like today. This preserves current behavior; a scheduled task that times out is not resumed automatically — resumption is user-initiated per the ticket.
- **Restate handler retries**: `max_attempts=0` on the inner `ctx.run("handle_interaction", ...)` means Restate won't retry the handler. That's intentional and unchanged.

---

## How to verify

**Before-state observation** (via existing test with monkey-patched 1s timeout):
- Run `just test-integration -- tests/integration/test_agent_interaction_errors.py::test_interaction_timeout_persists_timeout_error_code -v`.
- The pre-change assertion `error_row["error_code"] == "TIMEOUT"` passes; the row's `status` is `error` (verify by extending the SELECT to include status in a scratch run — this is the current state).

**After-state**:
- Same test file with the assertions updated (§Tests). `status = 'timed_out'`, partial rows survive, `is_continueable_response` is True.
- New `test_timed_out_interaction_is_continueable` proves the continue endpoint replays the preserved responses without discarding them.

**Full verification**: run `/code-checks` (skill) — this runs lint + api UT + common UT + integration tests in parallel per project convention.

**Frontend verification**: `pnpm nx test olly` for the component test suite; the component test for the new continue banner covers exists/substantive/wired/functional levels per `.claude/rules/verification.md`.

No manual e2e is required — integration tests are the verification per `.claude/CLAUDE.md`.
