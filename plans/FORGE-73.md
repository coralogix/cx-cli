# FORGE-73 — Preserve failed user turn across new interactions

## Problem recap (confirmed by code reading)

After a failed interaction, `_cleanup_failed_interaction` (`apps/api/src/api/agent/handlers/agent_interaction.py:568-603`) deletes every response for the failing `interaction_id` and inserts a **pseudo-response** carrying only the user's message (`_insert_pseudo_response`, `:505-565`) with `status=ERROR`, `response_data=NULL`, `user_message=<the failed message>`, `id` prefixed `pseudoresp_`.

When the user then sends **any** new message (`POST /v2/chats/{chat_id}/interactions/advanced`), `dispatch_interaction` (`apps/api/src/api/services/interactions_service.py:184-370`) drives the Restate handler, which calls `_handle_agent_interaction` → `_load_state` → `load_agent_run_input` (`apps/api/src/api/agent/response.py:745-786`). That calls `read_clean_responses` (`:556-582`), which walks the tail backwards and hard-deletes any response for which `is_continueable_response` (`:258-314`) returns `False`. `ERROR` short-circuits to `False` at `:282-283`, so the pseudo-response is deleted from the DB the instant a new message arrives — the failed user bubble disappears from chat rendering (`get_responses_by_chat_id`, `apps/api/src/api/repositories/chat_repository.py:255-294`) because the row is gone.

Second half of the problem (confirmed via code reading, not just from the ticket): `get_input_from_previous_responses` (`apps/api/src/api/agent/response.py:346-430`) appends `response.user_message` to the LLM input **unconditionally** when `user_message is not None`, regardless of `status`. So simply "stop deleting the ERROR row" is not enough — the failed user message would then be silently replayed to the model on every subsequent turn, which violates the ticket's success criteria. The fix has to split *"safe to keep in `team_responses`"* from *"safe to feed to the LLM"* as two separate predicates.

`update_interaction` (`apps/api/src/api/services/interactions_service.py:373-486`) — the retry/edit path — bypasses `read_clean_responses` entirely and deletes by `interaction_id` (with `ON DELETE CASCADE` from `interactions → responses`, `V1__initial_schema.sql:135-155`). It stays unaffected by any change we make here; the two paths are structurally decoupled.

## Constraints observed

- **Cannot run the project in this worktree.** No `docker`, no `just` binary available. Postgres/Redis/Restate/Kafka are needed for `just dev`. Recording the actual "before" chat state via a running UI is not possible here — verification is via unit + integration tests (`just test-api`, `just test-integration`) during implementation, which the CLAUDE.md `verification.md` rule already mandates as the source of truth for "done."
- The before-state is documented by code reading above: `SELECT * FROM team_responses WHERE chat_id = X AND status = 'error'` returns 0 rows after a subsequent POST today; the new integration test asserts the row survives after the fix.

## Fix

### File: `apps/api/src/api/agent/response.py`

1. **Add a preservation predicate** just above `read_clean_responses` (around line 555):

   ```python
   def _is_preserved_error_pseudo_response(response: ChatResponseRead) -> bool:
       """A trailing ERROR pseudo-response that carries a user-only turn.

       These rows are inserted by `_cleanup_failed_interaction` /
       `_insert_pseudo_response` when an interaction fails. They exist solely to
       keep the user's failed message visible in chat history — they must NOT be
       garbage-collected by `read_clean_responses` on the next interaction, but
       they must ALSO NOT be replayed to the LLM (see
       `get_input_from_previous_responses`).
       """
       return (
           response.status is ResponseStatus.ERROR
           and response.user_message is not None
           and response.response is None
       )
   ```

   Rationale for the exact field triad:
   - `status is ERROR` — obvious.
   - `user_message is not None` — this is what distinguishes the pseudo-response we care about from other ERROR rows (e.g. the pre-inserted `preresp_*` row that `run_structured_responses_call` flips to ERROR at `apps/api/src/api/agent/run_agent_utils.py:562-565`, which has `user_message=NULL`).
   - `response is None` — belt-and-braces: the pseudo-response by construction has `response_data=NULL`.
   - We deliberately do **not** match on `id LIKE 'pseudoresp_%'`. The field triad is the semantic condition; the id prefix is an implementation detail.

2. **Modify `read_clean_responses`** (`:573-577`) so that a trailing preserved ERROR pseudo-response is a **stop-the-cleanup boundary**, not a delete-and-continue candidate:

   ```python
   for response in reversed(previous_responses):
       if is_continueable_response(response):
           break
       if _is_preserved_error_pseudo_response(response):
           # Preserve the failed user-only turn; treat as boundary — anything
           # before it is real history to keep, so stop the cleanup here too.
           break
       response_ids_to_delete.append(response.id)
       saved_responses.pop()
   ```

   Why `break` (boundary) and not `continue`: `read_clean_responses`'s contract is "delete the *broken tail* and return the clean prefix." A preserved ERROR pseudo-response is a *legitimate* row that anchors real history — it is not a broken tail. Treating it as a boundary also means `saved_responses` (which is the returned prefix and gets fed into `build_history_with_compaction`) includes the ERROR row.

3. **Modify `get_input_from_previous_responses`** (at `:377`, top of the per-response loop) so an ERROR turn is skipped entirely on the LLM-replay side:

   ```python
   for response in previous_responses:
       # Preserved ERROR pseudo-responses live in team_responses so the failed
       # user bubble stays visible in chat history, but they must never be
       # replayed to the model — the user's message wasn't actually answered
       # and re-sending it would create phantom turns.
       if response.status is ResponseStatus.ERROR:
           continue
       if response.user_message is not None:
           ...
   ```

   This is the **only** place LLM input is built from persisted responses, so a single guard here covers every consumer (`build_history_with_compaction`, `agent_runner._sub_agent_input`, `memory_extractor._filter_input`, `scripts/generate_sub_agent_inputs.py`, `evals/scripts/export_chat_history.py`).

That's the entire production diff. `is_continueable_response` itself is **not** touched — its existing semantics ("can we resume from this?" → No for ERROR) are correct; we just don't let one broken predicate drive two independent policies.

### No changes needed in

- `_cleanup_failed_interaction` / `_insert_pseudo_response` — already inserts the pseudo-response with the right shape.
- `chat_repository.get_responses_by_chat_id` — already reads all rows regardless of status.
- `chat_service._convert_response_to_messages` (`apps/api/src/api/services/chat_service.py:238-293`) — already emits only a USER message when `response_data IS NULL`, so the preserved row renders as a user-only turn automatically.
- `stream_events._get_events_for_response` (`apps/api/src/api/agent/stream_events.py:1077-1102`) — already emits a `RunErrorEvent` for `is_top_level ERROR` rows; no change needed for the streaming path.
- `update_interaction` — decoupled (`chat_repository.delete_interactions` deletes by id/created_at with CASCADE); PATCH continues to replace the failed turn intentionally.
- `_load_latest_response_state` — the preserved ERROR row already carries forward `agent_context` / `agent_name` by design (see the `_insert_pseudo_response` docstring at `:519-525`). Under the fix, when the next interaction's `_load_latest_response_state` reads the tail, it finds the ERROR row; because its `interaction_id != run_context.interaction_id`, `agent_name` resets to `PRO_MAIN_AGENT` (`:210-211`), which is identical to today's behavior after `read_clean_responses` deletes the ERROR row and `_load_latest_response_state` falls through to the pre-error response. No functional difference.

## Order of changes

1. Add `_is_preserved_error_pseudo_response` helper.
2. Wire it into `read_clean_responses` (DB-preservation side).
3. Add the `ERROR` skip to `get_input_from_previous_responses` (LLM-replay side).
4. Add unit tests (below).
5. Add integration test (below).
6. Run `just lint-fix`, `just test-api`, `just test-integration` (via `/code-checks`).

Steps 2 and 3 must ship together — either one alone leaves the system in a worse state than today (step 2 alone replays a phantom user message to the LLM; step 3 alone silently drops turns that are still being deleted).

## Tests to add

### `apps/api/tests/ut/test_responses.py`

**A. `TestIsContinueableResponse` — direct predicate tests (currently missing entirely per grep of the tests dir).**
- `COMPLETED` / `RUN_COMPLETED` / `STOPPED` → `True`.
- `ERROR` → `False`.
- `IN_PROGRESS` with `response.output` containing a `function_call` matched by `tool_responses[*].call_id` → `True`.
- `IN_PROGRESS` with a `function_call` and no matching tool_response → `False`.
- `IN_PROGRESS` with an unexpected output type → `False` (log warning path).

**B. `TestIsPreservedErrorPseudoResponse` — the new predicate.**
- ERROR + user_message set + response None → `True`.
- ERROR + user_message None (e.g. pre_id row flipped to ERROR) → `False`.
- ERROR + user_message set + response not None → `False` (defensive; not a real state today).
- Non-ERROR statuses with user_message set → `False`.

**C. `TestReadCleanResponses` — this function currently has zero direct tests (it's only mocked at callsites).** Patch `read_top_level_chat_responses` and `delete_responses`, then assert:
- Trailing preserved ERROR pseudo-response is **not** in `delete_responses`'s call and **is** in the returned list.
- Trailing ERROR row without user_message (simulating a `preresp_*` failure) **is** deleted.
- Trailing non-ERROR non-continueable (e.g. IN_PROGRESS with tool_call mismatch) **is** deleted.
- Multiple consecutive trailing ERROR pseudo-responses: cleanup stops at the newest one; none are deleted. This confirms the chained-failure behavior called out in the ticket's success criteria.
- Preserved ERROR pseudo followed by a continueable row (impossible today because it's always trailing, but assert the boundary logic anyway) → nothing deleted, both returned.

**D. Extend `TestGetInputFromPreviousResponses`.**
- ERROR pseudo-response mid-history (surrounded by completed turns): the produced input list contains user_messages/response items from the surrounding turns but **not** the ERROR turn's `user_message`, `additional_context`, or `tool_responses`.
- ERROR pseudo-response as the last item in history: also entirely skipped.

**E. Extend `TestLoadAgentRunInput`.**
- Prior responses list ends with a preserved ERROR pseudo-response for a different `interaction_id`: `is_new_interaction` still evaluates `True`, `user_input` still populated from the pending message.

### `tests/integration/test_agent_interaction_errors.py`

Add a new test (mirrors the existing pattern of mocking `Runner.run_streamed`):

```
test_error_pseudo_response_survives_subsequent_interaction
```

1. Create a chat.
2. POST interaction #1 with `Runner.run_streamed` patched to raise (e.g. `RuntimeError("boom")`). Assert:
   - Response comes back / TerminalError propagates.
   - `SELECT id, status, user_message, response_data FROM team_responses WHERE chat_id = $1 AND status = 'error'` returns exactly one row with `user_message` set and `response_data` NULL.
3. POST interaction #2 with `Runner.run_streamed` patched to succeed (return a small streamed result). Assert:
   - The interaction #1 ERROR row is still present in `team_responses` (same query).
   - `GET /v2/chats/{chat_id}` returns *both* interaction #1's user-only turn *and* interaction #2's user+assistant pair, in creation order.
4. **LLM-input assertion:** capture the `input` argument to the `run_streamed` mock on the second POST and assert none of its user messages contain interaction #1's text. This is what proves the "still omits the errored turn entirely" success criterion.

### `tests/integration/test_interactions_route_v2.py` (or the errors file)

Add `test_patch_after_error_still_replaces_the_failed_turn`: repeat step 1 above, then `PATCH /v2/chats/{chat_id}/interactions/{failed_interaction_id}` with new content and assert the ERROR row is deleted (via CASCADE) and replaced by the new interaction's rows. This locks in the "PATCH is out of scope / unaffected" contract.

## Edge cases & risks

- **Chained failures** (fail → fail → fail): each `_cleanup_failed_interaction` deletes rows for its own `interaction_id` and inserts a fresh pseudo-response. Because our new `read_clean_responses` boundary logic stops at the first preserved ERROR row it encounters walking backwards, older ERROR pseudos are automatically untouched. Explicitly covered by test C above.

- **Compaction interacts cleanly.** `read_clean_responses` is scoped by `after_response_id` (the compaction boundary). If the ERROR row sits before the boundary it's not in scope; if it sits after, the boundary logic preserves it. Rendering reads all rows regardless of compaction, so the failed bubble shows in both cases. No new code needed but worth an assertion in the integration test if easy.

- **Memory extraction** (`memory_extraction.py` → `_filter_input` in `memory_extractor.py:243-284`): scans backwards for the newest `user_message` to identify "the current turn." If a preserved ERROR row sits in history, it has a `user_message` but no assistant reply; the LLM-replay fix in step 3 above already filters it out at the `get_input_from_previous_responses` layer, so memory extraction sees a clean history without phantom turns. Memory extraction only runs after successful interactions (`extract_memories` is dispatched from `handle_user_message`'s success path, `agent_interaction.py:855-870`), so a chat whose *latest* interaction errored never triggers extraction — the ERROR row only affects extraction when it is *older* history.

- **Sub-agent ERROR rows** (`is_top_level=False`, e.g. from `run_agent_utils.py:562-565`): `read_top_level_chat_responses` filters `is_top_level = TRUE`, so these never reach `read_clean_responses`. Untouched.

- **Structured-LLM pre_id → ERROR path** (`run_agent_utils.py:562-565`, has `is_top_level=True` when called from top-level structured calls, `user_message=NULL`): our new predicate returns `False` for this row (no `user_message`), so it continues to be deleted as before. Confirmed by test B above.

- **Agent-context deserialization drift**: covered under "No changes needed in `_load_latest_response_state`" above. Behavior is equivalent to today.

## Verification (run/check commands)

From the repo root of the worktree:

- Lint: `just lint` (per `apps/api/justfile:56`).
- Unit tests, scoped: `just test-api -k "responses or continueable or clean_responses or agent_interaction_errors"`.
- Integration tests, scoped: `just test-integration -k "interaction_errors or interactions_route_v2 or chats_route_v2"`.
- All checks together: run `/code-checks` (the repo's canonical verification skill, per `.claude/skills/run-code-checks/SKILL.md` and the mandatory `verification.md` rule).

**Before/after evidence** (documented in the integration test rather than manually — the environment can't run services):
- Before (today): `SELECT COUNT(*) FROM team_responses WHERE chat_id = X AND status = 'error'` = 0 after a subsequent successful POST.
- After (with fix): same query = 1, with `user_message` set and `response_data` NULL. Independently, the LLM-input capture assertion proves the failed message is not replayed to the model.

Artifacts (SQL query output snippets, `GET /v2/chats/{chat_id}` JSON) can be dumped to `.saga/artifacts/` from the integration test on failure for orchestrator upload; on success the test itself is the verification.

## Files touched (final list)

Production:
- `apps/api/src/api/agent/response.py` — add helper; modify two functions.

Tests:
- `apps/api/tests/ut/test_responses.py` — add classes A–E above.
- `tests/integration/test_agent_interaction_errors.py` — add `test_error_pseudo_response_survives_subsequent_interaction` and (optionally, if it fits the file's scope) the PATCH regression test; otherwise put PATCH regression in `tests/integration/test_interactions_route_v2.py`.

No new files, no deletes, no renames, no migrations, no CI changes, no frontend changes.
