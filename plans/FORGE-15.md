
# FORGE-15 — Verifier feedback retry (let the step fix its own output before needing a human)

## Goal & current behavior

Today the verifier seam in each step (triage / product_definition / technical_plan / implementation default `post_step`) handles a non-PASS verdict by either:

- **FAIL** → `record_verifier_fail()` in `src/saga/orchestrator/steps/generic.py` bumps `consecutive_failures[step.name]`, appends a `VERIFY_REJECT` `StepRecord`, then either `mark_locally_failed()` (budget exhausted) or `loop_back()` (resets `stage` to `ENTERED`, the next tick re-runs the step from scratch with `format_prior_attempts(...)` injected into the prompt).
- **INCONCLUSIVE** → goes straight to `needs_human()` with `Pause.NEEDS_INPUT` (triage / product_definition / technical_plan), or to `publish_outcome(..., level=APPROVE)` + `Pause.GATE` (default `post_step` in `step.py`, used by implementation). **Never counted toward the budget; never given a chance to fix itself.**

Both paths lose the step's working session context — `loop_back` keeps `session_id` but the next tick sends the standard step-entry prompt again, and the human-pause paths just stop.

We want: on **FAIL or INCONCLUSIVE**, inject the verifier's `notes` straight into the live session as a follow-up turn so the agent calls `record_<step>(...)` again with corrected output, then re-verify. Use the existing `consecutive_failures` / `step.max_attempts()` budget. After **3** consecutive non-PASS verdicts → escalate (FAIL → `mark_locally_failed`; INCONCLUSIVE → `needs_human` / `Pause.NEEDS_INPUT` per step). Session loss or feedback-turn error → fall back to `loop_back()` (today's behavior).

Cross-reference with FORGE-11 (graceful shutdown): in-process sessions are not restored across orchestrator restart, so a session captured in `SessionManager._sessions` may be gone after a restart even though `TaskState.session_id` is persisted. The `loop_back` fallback is the failure path that already handles this, so this change does not need to wait for FORGE-11.

## Run / verify

Per `CLAUDE.md`:
- **Tests:** `uv run pytest tests/ -q` (`just test`). Run single file/test with `uv run pytest tests/test_flow_generic.py::<name>`.
- **Lint / types:** `uv run ruff check && uv run ruff format --check && uv run ty check` (`just lint`).
- **Run orchestrator:** `uv run saga run` (requires `LINEAR_OAUTH_TOKEN`, Slack creds, real Linear ticket — not exercised here; the unit-test suite is the verification surface for this change).

**Before-state captured in this environment:**
- `uv run pytest tests/ -q` → **705 passed** (with a pre-existing `RuntimeWarning: coroutine 'AsyncMockMixin._execute_mock_call' was never awaited` from `tests/test_technical_plan.py` — unrelated).
- `uv run ruff check && uv run ruff format --check && uv run ty check` → all clean.

There is no way to reproduce the user-visible bug end-to-end in this worktree (it requires a real Linear ticket plus a Claude Code session running the verifier). The verification surface is therefore the unit-test suite, exercising `generic.record_verifier_fail` and each step's `post_step` against fake sessions and fake verdicts.

## Design

### Where the loop lives

The loop lives **inside `record_verifier_fail()` in `src/saga/orchestrator/steps/generic.py`**. The function changes from "record + decide loop_back-vs-escalate" to:

```
while verdict.result is not PASS:
    record this VERIFY_REJECT; bump consecutive_failures
    if attempts >= step.max_attempts():
        if verdict is FAIL: mark_locally_failed()         # caller bails on FAIL return
        # INCONCLUSIVE: do NOT escalate here — return the verdict; caller does step-specific escalation
        return verdict
    session = ctx.deps.session_mgr.get(ctx.task.id)
    if session is None:
        await loop_back(ctx, step); return None           # fallback (today's behavior)
    feedback = _build_verifier_feedback_message(step.name, verdict)
    nudge = await session.send(feedback)
    if nudge.session_id is not None:
        await task_state_repo.update_state(ctx.task.id, session_id=nudge.session_id)
    if nudge.event == "failed":
        await ctx.deps.session_mgr.close(ctx.task.id)
        await loop_back(ctx, step); return None           # fallback
    new_verdict = await step.verify(ctx, out)
    if new_verdict is None:
        await loop_back(ctx, step); return None           # verifier infra failure mid-loop
    verdict = new_verdict
return verdict   # PASS
```

The function now needs `out: StepOutcome` so it can re-call `step.verify(ctx, out)` after each feedback turn. Every step's `verify()` already re-reads the latest structured result from `TaskState` (triage / PD / tech_plan) or recomputes the diff (implementation), so the same `out` works across iterations.

### Function contract (new)

```python
async def record_verifier_fail(
    ctx: StepCtx, step: "Step", out: StepOutcome, verdict: Verdict
) -> Verdict | None:
    """Drive the verify-feedback retry loop on a non-PASS verdict.

    Returns:
      - Verdict with result=PASS  → caller proceeds to its normal gate / publish / advance path.
      - Verdict with result=FAIL  → budget exhausted; `mark_locally_failed` already triggered;
                                    caller must just return.
      - Verdict with result=INCONCLUSIVE → budget exhausted; caller runs its step-specific
                                    inconclusive escalation (needs_human / NEEDS_INPUT for
                                    triage / product_definition / technical_plan; APPROVE-pause
                                    via publish_outcome for default post_step).
      - None → session lost or feedback turn errored; `loop_back` already called; caller bails.
    """
```

### Feedback message format

Matches the ticket's spec ("Q3"): short directive, no `Verdict` envelope.

```python
def _build_verifier_feedback_message(step_name: str, verdict: Verdict) -> str:
    framing = (
        "Your output was reviewed and rejected by the verifier."
        if verdict.result is VerdictResult.FAIL
        else "Your output could not be independently verified."
    )
    notes = verdict.notes or "(no details)"
    return (
        f"{framing}\n\n"
        f"Feedback: {notes}\n\n"
        f"Please address these issues and call `record_{step_name}(...)` again "
        f"with the revised output. Do not output any other text before the tool call."
    )
```

Mirrors the shape of the existing `record_*` nudge in `GenericAgentStep.work()`.

### Per-step `post_step` reshape

Every callsite of `record_verifier_fail()` follows the same shape after the change:

```python
verdict = await self.verify(ctx, out)
if verdict is not None and verdict.result is not VerdictResult.PASS:
    verdict = await record_verifier_fail(ctx, self, out, verdict)
    if verdict is None:
        return                                          # loop_back fallback
    if verdict.result is VerdictResult.FAIL:
        return                                          # mark_locally_failed already triggered
    if verdict.result is VerdictResult.INCONCLUSIVE:
        # step-specific inconclusive escalation (the existing block, unchanged):
        ...
        return
# PASS (or first-call None) → fall through to gate / publish
```

Concretely:

- **`src/saga/orchestrator/steps/triage/__init__.py`** (~line 181): drop the standalone `INCONCLUSIVE` branch above the gate; let it run only after `record_verifier_fail` returns INCONCLUSIVE. Reword the pre-call Linear comment `🔄 Verifier found issues in triage — {notes}\nRe-running triage.` → `… Trying to fix with feedback.`
- **`src/saga/orchestrator/steps/product_definition/__init__.py`** (~line 160): same shape; reword the `Re-running product definition.` trailer.
- **`src/saga/orchestrator/steps/technical_plan/__init__.py`** (~line 212): same shape; reword the `Re-running the technical plan.` trailer.
- **`src/saga/orchestrator/steps/step.py`** `Step.post_step()` (default — used by `implementation`): replace the INCONCLUSIVE-→-pause-APPROVE branch with the same call-then-dispatch pattern. The post-loop INCONCLUSIVE handler keeps the existing `publish_outcome(... APPROVE)` + `Pause.GATE` behavior, but only fires when the budget is exhausted (i.e. INCONCLUSIVE persisted through 3 feedback turns).

### Why one function instead of split helpers

- Splitting the loop into a separate `verify_with_feedback` helper would still leak `out` and `step.verify` into `generic.py`, and would force every step's `post_step` to know about both helpers. One function with one return contract is the smaller surface.
- The "INCONCLUSIVE counts toward budget" behavior is the ticket's explicit ask ("3 consecutive retries" applies to both kinds). Direct-to-needs_human INCONCLUSIVE paths (`triage` / `product_definition` / `technical_plan`) move behind the budget; that's intentional and documented in the function docstring.
- The `loop_back()` fallback for session loss / feedback-turn error preserves today's "no live session ⇒ retry from ENTERED with prior-attempts injected" semantics so an orchestrator restart between ticks (or a session that something else closed) keeps working.

### Visibility / notifications

- The pre-call `🔄 Verifier found issues …` Linear comment is posted **once** per `post_step` invocation (no change to call frequency). Each iteration's `StepRecord` (`verdict.notes`, `attempt`) captures the per-attempt detail in `step_records` — the comment surface stays one ticket comment per post_step call.
- For `technical_plan`, `SessionManager._make_on_assistant_text` already mirrors agent text to Slack — the feedback-turn assistant text shows up in the thread automatically. Other steps stay silent (matches current behavior). No new notifier wiring needed.

## Files to change

1. **`src/saga/orchestrator/steps/generic.py`** — rewrite `record_verifier_fail` per the contract above; add `_build_verifier_feedback_message` private helper. Update the function-level docstring (remove the "INCONCLUSIVE must NOT call this" warning; replace with the new contract). Import `VerdictResult` and `StepOutcome` if not already in scope.
2. **`src/saga/orchestrator/steps/step.py`** — update the default `Step.post_step` to the new call pattern; thread `out` through to `record_verifier_fail`; move the INCONCLUSIVE-→-GATE-pause logic so it only fires on the post-loop returned verdict.
3. **`src/saga/orchestrator/steps/triage/__init__.py`** — adjust `post_step`. Existing direct-INCONCLUSIVE→needs_human block becomes the post-loop branch. Reword comment text.
4. **`src/saga/orchestrator/steps/product_definition/__init__.py`** — same.
5. **`src/saga/orchestrator/steps/technical_plan/__init__.py`** — same. (Note: the `record_verifier_fail` call is inside the existing FAIL block; this change moves it into a unified non-PASS dispatch.)
6. **`tests/test_flow_generic.py`** — update the four existing `record_verifier_fail` tests to pass `out` and configure `session_mgr.get` → `None` so they take the fallback path (preserves their current assertion intent). Add:
   - `test_record_verifier_fail_sends_feedback_and_returns_pass_when_reverified_pass` — fake session returns a `SessionTurnOutcome("completed", session_id=…)` for the feedback turn; `step.verify` patched to return PASS on the second call; function returns PASS; one VERIFY_REJECT record appended; counter bumped once; `mark_failed` not called.
   - `test_record_verifier_fail_feedback_loops_until_budget_then_escalates` — `step.verify` keeps returning FAIL; after `max_attempts` failures, `mark_locally_failed` is called and final return is the FAIL verdict; that many VERIFY_REJECT records appended.
   - `test_record_verifier_fail_inconclusive_loops_with_feedback` — initial verdict INCONCLUSIVE; second verify returns PASS; PASS returned; counter bumped once.
   - `test_record_verifier_fail_inconclusive_at_budget_returns_inconclusive_no_mark_failed` — INCONCLUSIVE the whole way; on budget exhaustion the function does NOT call `mark_locally_failed`; returns the INCONCLUSIVE verdict.
   - `test_record_verifier_fail_feedback_turn_errors_falls_back_to_loop_back` — `session.send` returns `event="failed"`; `session_mgr.close` called; `loop_back` triggered; function returns None.
   - `test_record_verifier_fail_reverify_returns_none_falls_back_to_loop_back` — `step.verify` returns None on the post-feedback re-verify; `loop_back` triggered; returns None.
7. **`tests/test_technical_plan.py`** — keep `test_post_step_verifier_fail_records_and_loops_below_budget` and `test_post_step_verifier_fail_escalates_at_budget` working by configuring `session_mgr.get` → None for the fallback path. Add:
   - `test_post_step_verifier_feedback_then_pass_advances` — session present; first verify FAIL, then PASS after feedback turn; ticket advances; counter bumped 1; one VERIFY_REJECT record.
   - `test_post_step_verifier_inconclusive_at_budget_pauses_for_input` — INCONCLUSIVE 3×; final state has `pause=NEEDS_INPUT`; `needs-human` label applied via tracker.add_label.
8. **`tests/test_triage_step.py`** — analogous: rebase the two `verifier_fail` tests on the no-session fallback; add a feedback-then-PASS test and an INCONCLUSIVE-at-budget-pauses test that mirrors the triage post_step's needs_human path.
9. **`tests/test_product_definition_step.py`** (verify exact filename — also a `tests/test_flow_*` if present) — same additions.
10. **`tests/test_flow_generic.py`** — the existing `test_post_step_verify_inconclusive_pauses` default-`post_step` test asserts `pause=GATE` on first INCONCLUSIVE — update it to thread through `session_mgr.get` → None so it takes the budget-exhaustion path and still pauses on `Pause.GATE` (now after the loop). Add a feedback-then-PASS variant.

## Order of changes

1. Rewrite `record_verifier_fail()` + add `_build_verifier_feedback_message()` in `generic.py`.
2. Update the default `Step.post_step` in `step.py` to the new call pattern; pass `out` through.
3. Update `triage`, `product_definition`, `technical_plan` `post_step`s — straightforward shape change.
4. Update existing tests to pass `out` and configure `session_mgr.get` → None (preserves the current assertion intent on the fallback path).
5. Add new tests for the feedback-loop happy paths and the INCONCLUSIVE-at-budget paths.
6. `uv run ruff check && uv run ruff format && uv run ty check`, then `uv run pytest tests/ -q`.

## Edge cases and risks

- **Agent doesn't call `record_*` on the feedback turn.** `ts.<result_attr>` stays unchanged; next `step.verify(ctx, out)` evaluates the unchanged output and returns the same verdict — burns a budget slot. Acceptable — eventual `mark_locally_failed` / `needs_human` after `max_attempts`. We do **not** add a separate "did the result change?" check; the verifier's verdict on still-stale output is the signal we need.
- **Re-verify returns `None` (verifier infra failure mid-loop).** Treat as inconclusive infra: `loop_back` fallback. The first-call `None` (i.e. verify returned None before the loop ever entered) is unchanged — caller still falls through.
- **`session.send` triggers a long agent turn.** Blocks the `_drive` call (and therefore the orchestrator tick); the existing architecture already accepts this — every `work()` turn is awaited inline. No new concern.
- **`session.send` may not actually be a fresh `query` mid-session.** Reading `ClaudeAgentSession.send`: the first call calls `client.connect(text)`, subsequent calls call `client.query(text)`. By the time `record_verifier_fail` runs, the session is already connected from `work()` so `send` will use `query`, matching the ticket's spec ("Q1").
- **`technical_plan` mirrors agent text to Slack via `_make_on_assistant_text`.** The feedback turn's mid-loop assistant text will appear in the Slack thread, which is desirable visibility. Other steps stay silent (matches current behavior).
- **`consecutive_failures` budget reset on human resume** (`_unblock` already clears it, `consume_thread_reply` clears it for FAILED) — no behavior change. A re-dispatched step gets a fresh budget, same as today.
- **Orchestrator restart between work and verify** (overlaps FORGE-11): `SessionManager._sessions` is in-process; a restart wipes it. `session_mgr.get` then returns `None`, the loop_back fallback fires, and the next tick re-enters the step from scratch with prior-attempts injected — same as today's behavior. This change does not regress any restart behavior.
- **Concurrent execution.** The whole loop runs in one `_drive` call inside one background asyncio task already registered in `running_agents`; no second dispatch can race it. No locking needed.
- **`AsyncMock` defaults in tests.** `AsyncMock().get(task_id)` returns an `AsyncMock` (truthy) by default, which would be mistaken for a live session and break the existing fallback-path tests. Tests must explicitly set `session_mgr.get = MagicMock(return_value=None)` for the fallback path or `MagicMock(return_value=fake_session)` for the feedback path. Call this out in the test helper docstring.
- **Comment text drift.** The pre-call `🔄 Verifier found issues … — Re-running …` comment is now slightly misleading ("Re-running" → "Trying to fix with feedback"). Reword in the same edit.
- **No code change in `loop.py` / `runner.py`.** `consume_thread_reply` and the `_drive` stage machine keep working unchanged: a human thread reply still routes through `reply_text` (a separate code path); the feedback loop here is entirely intra-`post_step`.

## Verification

1. `uv run ruff check && uv run ruff format --check && uv run ty check` → all clean.
2. `uv run pytest tests/ -q` → baseline 705 + the new tests pass, no regressions in existing tests.
3. Spot-check each changed `post_step`: PASS fall-through, `Verdict.FAIL` early return (escalation handled inside `record_verifier_fail`), `Verdict.INCONCLUSIVE` post-loop step-specific escalation, `None` early return (loop_back).
4. Capture the after-state `pytest -q` summary as `.saga/artifacts/pytest_after.txt` for the implementation phase to attach to Linear / the PR.
