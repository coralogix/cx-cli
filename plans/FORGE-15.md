# FORGE-15 — Verifier feedback retry (let the step fix its own output)

## Goal & current behavior

Today the verifier seam in each step (triage / product_definition / technical_plan / implementation default `post_step`) handles a non-PASS verdict by either:

- **FAIL** → `record_verifier_fail()` in `src/saga/orchestrator/steps/generic.py` bumps `consecutive_failures[step.name]`, appends a `VERIFY_REJECT` `StepRecord`, then either `mark_locally_failed()` (budget exhausted) or `loop_back()` (resets `stage` to `ENTERED`, next tick re-runs the step from scratch with `format_prior_attempts(...)` injected into the prompt).
- **INCONCLUSIVE** → goes straight to `needs_human()` with `Pause.NEEDS_INPUT` (triage / product_definition / technical_plan), or to `publish_outcome(..., level=APPROVE)` + `Pause.GATE` (default `post_step` in `step.py`, used by implementation). **Never counted toward the budget; never given a chance to fix itself.**

Both paths lose the step's working session context — `loop_back` keeps `session_id` but the next tick sends the standard step-entry prompt again, and the human-pause paths just stop.

We want: on **FAIL or INCONCLUSIVE**, inject the verifier's `notes` straight into the live session as a follow-up turn so the agent calls `record_<step>(...)` again with corrected output, then re-verify. Use the existing `consecutive_failures` / `step.max_attempts()` budget. After **3** consecutive non-PASS verdicts → escalate (FAIL → `mark_locally_failed`; INCONCLUSIVE → `needs_human` / `Pause.NEEDS_INPUT`). Session loss or feedback-turn error → fall back to `loop_back()` (today's behavior).

## Run / verify

Per `CLAUDE.md`:
- **Tests:** `uv run pytest tests/ -q` (`just test`). Run single file/test with `uv run pytest tests/test_flow_generic.py::<name>`.
- **Lint:** `uv run ruff check && uv run ruff format --check && uv run ty check` (`just lint`).
- **Run orchestrator:** `uv run saga run` (requires Linear OAuth + Slack creds — not exercised here; the unit-test suite is the verification surface for this change).

**Before-state captured:** `uv run pytest tests/ -q` → **705 passed**, no warnings beyond the pre-existing `save_plan` mock-coroutine notice. `uv run ruff check && ruff format --check && ty check` → all clean.

There is no way to reproduce the user-visible bug end-to-end in this worktree (it requires a real Linear ticket + Claude Code session running the verifier). The verification surface for this change is therefore the unit-test suite, exercising `generic.record_verifier_fail` and each step's `post_step` against fake sessions / verdicts.

## Design

### Where the loop lives

The loop lives **inside `record_verifier_fail()` in `src/saga/orchestrator/steps/generic.py`**. The function changes from "record + decide loop_back-vs-escalate" to:

```
while verdict.result is not PASS:
    record this VERIFY_REJECT, bump consecutive_failures
    if attempts >= step.max_attempts():
        escalate (verdict-kind-specific) and return verdict
    session = ctx.deps.session_mgr.get(ctx.task.id)
    if session is None:
        await loop_back(ctx, step); return None       # fallback (today's behavior)
    feedback = _build_verifier_feedback_message(step.name, verdict)
    nudge = await session.send(feedback)
    persist nudge.session_id if present
    if nudge.event == "failed":
        await session_mgr.close(task.id)
        await loop_back(ctx, step); return None       # fallback
    new_verdict = await step.verify(ctx, out)
    if new_verdict is None:
        await loop_back(ctx, step); return None       # infra fallback
    verdict = new_verdict
return verdict   # PASS
```

The function now needs `out: StepOutcome` so it can re-call `step.verify(ctx, out)` after each feedback turn. Each step's `verify()` already re-reads the latest result from `TaskState` (or recomputes the diff for implementation), so this works without further plumbing.

### Function contract (new)

```python
async def record_verifier_fail(
    ctx: StepCtx, step: "Step", out: StepOutcome, verdict: Verdict
) -> Verdict | None:
    """Drive the verify-feedback retry loop.

    Returns:
      - Verdict with result=PASS  → caller proceeds to its normal gate/publish/advance path.
      - Verdict with result=FAIL  → budget exhausted; `mark_locally_failed` already triggered;
                                    caller must just return.
      - Verdict with result=INCONCLUSIVE → budget exhausted; caller runs its step-specific
                                    inconclusive escalation (needs_human/NEEDS_INPUT for
                                    triage/product_definition/technical_plan; APPROVE-pause
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

(Reusing the same shape as the existing `record_*` nudge message in `GenericAgentStep.work()`.)

### Per-step `post_step` reshape

Every callsite of `record_verifier_fail()` becomes:

```python
verdict = await self.verify(ctx, out)
if verdict is not None and verdict.result is not VerdictResult.PASS:
    verdict = await record_verifier_fail(ctx, self, out, verdict)
    if verdict is None:
        return                                          # loop_back fallback
    if verdict.result is VerdictResult.FAIL:
        return                                          # mark_locally_failed already triggered
    if verdict.result is VerdictResult.INCONCLUSIVE:
        # step-specific inconclusive escalation (the existing code, unchanged):
        ...
        return
# PASS or None (infra failure on first verify) → fall through to gate/publish
```

Concretely:

- **`src/saga/orchestrator/steps/triage/__init__.py`** (~line 181): drop the standalone `INCONCLUSIVE` branch above the gate; let it run only after `record_verifier_fail` returns INCONCLUSIVE. Keep the existing pre-call `tracker.add_comment("🔄 Verifier found issues in triage — …\nRetrying with feedback.")` but reword the trailing text from "Re-running triage." → "Trying to fix with feedback." (this comment now precedes the feedback loop rather than a loop_back).
- **`src/saga/orchestrator/steps/product_definition/__init__.py`** (~line 160): same shape; reword the "Re-running product definition." comment.
- **`src/saga/orchestrator/steps/technical_plan/__init__.py`** (~line 212): same shape; reword the "Re-running the technical plan." comment.
- **`src/saga/orchestrator/steps/step.py`** `Step.post_step()` (default — used by `implementation`): replace the INCONCLUSIVE-→-pause-APPROVE branch with the same call-then-dispatch pattern. The post-loop INCONCLUSIVE handler keeps the existing publish_outcome / `Pause.GATE` behavior, but only fires when the budget is exhausted.

### Pre-loop comment / Slack posting

The existing `🔄 Verifier found issues …` Linear comment is posted **once** at the top of each `post_step` (no change to call frequency). Each verifier attempt's `StepRecord` (with `verdict.notes` and `attempt`) already captures the per-attempt detail for the records log; the comment surface stays as one ticket comment per `post_step` invocation. The per-attempt Slack thread message comes for free from the existing `_make_on_assistant_text` callback wired in `SessionManager` for `technical_plan` (the agent's text mid-loop is mirrored to Slack). No new notifier path needed.

### Why not split into helpers / why localized

- Splitting the loop into a separate "`verify_with_feedback`" helper would still leak `out` and `step.verify` into `generic.py`, and would force every step's `post_step` to know about both helpers. One function with one return contract is the smaller surface.
- The `INCONCLUSIVE`-counts-toward-budget behavior is the user-requested ticket scope ("3 consecutive retries"). Existing call paths that previously skipped the counter on INCONCLUSIVE (`triage`/`product_definition`/`technical_plan` direct-needs_human) will now count INCONCLUSIVE — this is intentional and documented in the function docstring.
- The `loop_back()` fallback for session loss / feedback-turn errors preserves today's "no live session ⇒ retry from ENTERED" semantics, so an orchestrator restart between ticks (or a session that the user manually closed) keeps working.

## Files to change

1. **`src/saga/orchestrator/steps/generic.py`** — rewrite `record_verifier_fail` per the contract above; add `_build_verifier_feedback_message` private helper. Update the function-level docstring (remove the "INCONCLUSIVE must NOT call this" warning; replace with the new contract). Imports: add `VerdictResult` (already imported indirectly) and the `StepOutcome` type from `saga.orchestrator.steps.deps`.
2. **`src/saga/orchestrator/steps/step.py`** — update the default `Step.post_step` to use the new return contract. Pass `out` through. Move the INCONCLUSIVE-→-GATE-pause logic to run only on the post-loop returned verdict.
3. **`src/saga/orchestrator/steps/triage/__init__.py`** — adjust `post_step` to the new call pattern. The existing direct-INCONCLUSIVE→needs_human block becomes the post-loop branch.
4. **`src/saga/orchestrator/steps/product_definition/__init__.py`** — same.
5. **`src/saga/orchestrator/steps/technical_plan/__init__.py`** — same. (Note: the existing pre-comment text "Re-running …" is updated to "Trying to fix with feedback.")
6. **`tests/test_flow_generic.py`** — new tests; update existing four `record_verifier_fail` tests to pass `out` and assert the new behavior:
   - `test_record_verifier_fail_below_budget_with_no_session_loops_back` — session_mgr.get returns None → `loop_back` fallback; `stage == ENTERED`; **mark_failed not called**.
   - `test_record_verifier_fail_sends_feedback_and_returns_pass_when_reverified_pass` — session returns a `SessionTurnOutcome("completed", ...)` for the feedback turn; `step.verify` is patched to return PASS on the second call; function returns the PASS verdict; counter bumped once; one VERIFY_REJECT record appended; `mark_failed` not called.
   - `test_record_verifier_fail_feedback_loops_until_budget_then_escalates` — `step.verify` keeps returning FAIL; after `max_attempts` failures, `mark_locally_failed` is called and final return is the FAIL verdict; counter == max_attempts; that many VERIFY_REJECT records appended.
   - `test_record_verifier_fail_inconclusive_loops_with_feedback` — initial verdict is INCONCLUSIVE; on second verify returns PASS; PASS verdict returned; counter bumped once.
   - `test_record_verifier_fail_inconclusive_at_budget_returns_inconclusive_no_mark_failed` — INCONCLUSIVE the whole way; on budget exhaustion the function does NOT call `mark_locally_failed` (the caller's step-specific path handles it); returns the INCONCLUSIVE verdict.
   - `test_record_verifier_fail_feedback_turn_errors_falls_back_to_loop_back` — `session.send` returns `event="failed"`; session_mgr.close called; loop_back triggered; function returns None.
   - `test_record_verifier_fail_reverify_returns_none_falls_back_to_loop_back` — `step.verify` returns None on the post-feedback re-verify; loop_back triggered; returns None.
   - Keep `test_record_verifier_fail_stores_certainty` and `test_record_verifier_fail_no_notes` working by configuring `session_mgr.get` → None (so they take the fallback path the same way they used to with `loop_back`); names rename to reflect the path.
7. **`tests/test_technical_plan.py`** — keep `test_post_step_verifier_fail_records_and_loops_below_budget` working by configuring `session_mgr.get` → None for the fallback path. Add:
   - `test_post_step_verifier_feedback_pass_then_advances` — session present; first verify FAIL, then PASS after feedback turn; ticket advances; counter bumped 1; one VERIFY_REJECT record.
   - `test_post_step_verifier_inconclusive_at_budget_pauses_for_input` — INCONCLUSIVE 3×, final state has `pause=NEEDS_INPUT`, `needs-human` label applied.
8. **`tests/test_triage_step.py`** — analogous: add a feedback-then-PASS test and an INCONCLUSIVE-at-budget-pauses test that mirrors the triage post_step's needs_human path. Keep the existing two `verifier_fail` tests by routing them through the no-session fallback.
9. **`tests/test_product_definition_step.py`** (if present — verify with `tests/test_product_definition.py` glob) — same additions.
10. **`tests/test_flow_generic.py`** default-`post_step` INCONCLUSIVE test (`test_post_step_verify_inconclusive_pauses`) — extend with a feedback-then-PASS variant; update the existing test to drive through `session_mgr.get` → None so it still pauses on INCONCLUSIVE after the budget completes.

## Order of changes

1. Rewrite `record_verifier_fail()` + add `_build_verifier_feedback_message()` in `generic.py`.
2. Update the default `Step.post_step` in `step.py` to the new call pattern; pass `out` through.
3. Update `triage`, `product_definition`, `technical_plan` `post_step`s — straightforward shape change.
4. Update existing tests to pass `out` and configure `session_mgr.get` → None (preserves current assertion intent on the fallback path).
5. Add new tests for the feedback-loop happy paths and INCONCLUSIVE budget exhaustion.
6. `uv run ruff check && uv run ruff format && uv run ty check` then `uv run pytest tests/ -q`.

## Edge cases and risks

- **Agent doesn't call `record_*` on the feedback turn.** `ts.<result_attr>` stays the same; next `step.verify(ctx, out)` evaluates the unchanged output and returns the same verdict, burning a budget slot. Acceptable — eventual `mark_locally_failed` / `needs_human` after `max_attempts`. (We do **not** add a separate "did the result change?" check; the verifier's verdict on the still-stale output is the signal we need.)
- **Re-verify returns `None` (verifier infra failure mid-loop).** Treat as inconclusive infra, `loop_back` fallback. Same semantic as today's "verify returned None" path: caller-side flow that treated `None` as "no verdict; proceed" is preserved on the *first* call (before the loop is entered).
- **`session.send` triggers a long agent turn.** This blocks the `_drive` call (and therefore the orchestrator tick); the orchestrator tick architecture already accepts this — every `work()` turn is awaited inline. No new concern.
- **`technical_plan` mirrors agent text to Slack via `_make_on_assistant_text`.** The feedback turn's mid-loop assistant text will appear in the thread, which is desirable visibility. Other steps stay silent (matches current behavior).
- **`consecutive_failures` budget reset on human resume** (`_unblock` already clears it) — no behavior change. A re-dispatched step (via thread reply) gets a fresh budget, same as today.
- **Concurrent execution.** The whole loop runs in one `_drive` call inside one background asyncio task already registered in `running_agents`, so no second dispatch can race it. No locking needed.
- **Tests that mock `session_mgr` via `AsyncMock()`.** `AsyncMock().get(task_id)` returns an `AsyncMock` (truthy) by default, which would be mistaken for a live session. Tests must explicitly set `session_mgr.get = MagicMock(return_value=None)` to take the fallback path or `MagicMock(return_value=fake_session)` for the feedback path. Document this in the test helper.
- **Comment text drift.** The pre-call `🔄 Verifier found issues … — Re-running …` comment text is now slightly misleading ("Re-running" → "Trying to fix with feedback"). Reword in the same edit.

## Verification

1. `uv run ruff check && uv run ruff format --check && uv run ty check` → all clean.
2. `uv run pytest tests/ -q` → 705 (baseline) + new tests pass, no regressions in existing tests.
3. Spot-check the changed files visually for the loop contract: each step's `post_step` either takes the PASS fall-through, the `Verdict.FAIL` early return (escalation handled), the `Verdict.INCONCLUSIVE` post-loop branch, or the `None` early return (loop_back).

**After-state observation:** rerun the test suite to confirm `705 + N_new` passing; capture the `pytest -q` output as the "after" artifact under `.saga/artifacts/`.
