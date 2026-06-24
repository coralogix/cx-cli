# FORGE-9 — Prevent silent "Reached review with no PR to monitor" failure

## Goal

When a task reaches the `pr_review` phase (Linear status `In Review`) without a PR in `TaskState.prs`, do **not** silently mark it `Pause.FAILED` with the terse "Reached review with no PR to monitor." reason. Instead, gate at step entry: attempt PR recovery from GitHub; if still nothing, pause with a clear, human-actionable `Pause.NEEDS_INPUT` so the human can act in Slack/Linear.

## Approach: `pre_step` gate on `PrReviewStep`

Of the three options in the ticket I'm picking **the pre-step gate** (option 1) because:

- The step engine already has the perfect seam: `runner.py` runs `step.pre_step()` on `Stage.ENTERED`, and a `PAUSE` decision funnels through `pause_for_input` → `needs_human` (label, Linear comment, Slack `on_needs_human`, `Pause.NEEDS_INPUT`). That semantic — "I cannot proceed until a human gives me input" — fits "no PR to monitor" exactly. `Pause.FAILED` is the agent-error class; using it here is the bug.
- Recovery (option 2) is already attempted inside `poll()` (`pr_monitor.py:126`). The gap is not that recovery doesn't happen — it's that when recovery fails we drop into the wrong terminal failure path. So this is fundamentally a pause-class / messaging problem, not a recovery-timing problem.
- `NEEDS_INPUT` is resumable via Slack thread reply (`loop.py:236` allows `Pause.GATE | FAILED | NEEDS_INPUT`), so a human can answer "the PR is #123" or just "drop it" and the task re-runs `pre_step`. Re-running re-attempts recovery — exactly what we want.
- The work inside `poll()` stays unchanged, so all the existing PR-review behavior (merge/close/CI/conflict) is untouched.

Option 3 (explicit validation in implementation's `post_step`) doesn't fix the case the ticket is actually about — a human or automation manually dragging a card to `In Review` without going through implementation. The `pre_step` gate catches both that case and the "implementation didn't open a PR" case in one place.

## Files to change

### 1. `src/saga/orchestrator/steps/review/pr_monitor.py` — make recovery callable from siblings

`PrMonitor._recover_prs_from_github` already does exactly what `PrReviewStep.pre_step` needs (read branch_name, query GitHub per repo, persist what's found). Two options: call it through `pr_monitor` directly with the underscore, or rename to drop the underscore now that it has an external caller in a sibling module.

**Rename `_recover_prs_from_github` → `recover_prs_from_github`** (drop leading underscore). It is being promoted from a poll-time helper to a documented entry-recovery hook. Update the single call site inside `poll()` (line 126).

No behavior change in the function itself. Keep the existing per-repo try/except and the "no branch_name → empty" early return.

### 2. `src/saga/orchestrator/steps/review/step.py` — add `pre_step` override

Override `pre_step(self, ctx)` on `PrReviewStep` with a deterministic (non-LLM) implementation:

```python
async def pre_step(self, ctx: StepCtx) -> PreAssessment:
    ts = await task_state_repo.get(ctx.task.id) or TaskState()
    if ts.prs:
        return PreAssessment(decision=PreDecision.PROCEED)

    monitor = ctx.deps.pr_monitor
    if monitor is None:
        # No GitHub configured at all → let work() handle the no-monitor path.
        return PreAssessment(decision=PreDecision.PROCEED)

    recovered = await monitor.recover_prs_from_github(ctx.task)
    if recovered:
        return PreAssessment(decision=PreDecision.PROCEED)

    reason = (
        "Reached In Review with no PR to monitor and no open PR found on "
        f"GitHub for branch `{ts.branch_name or '(unset)'}`. "
        "Open the PR (or link it via the branch) and reply here to resume; "
        "or move the ticket back to Implementation to retry."
    )
    return PreAssessment(decision=PreDecision.PAUSE, reason=reason)
```

Notes / invariants:

- Re-reads `TaskState` from the store instead of trusting `ctx.ts` — the snapshot can be stale (open_pr writes during the agent turn), and the implementation step's `post_step` advanced us here moments ago, so we want the freshest read.
- Pure code — does **not** ship a `pre_step.md`. The base-class `Step.pre_step` (which is the LLM pre-assessor) is bypassed entirely; overriding `pre_step` directly is the documented escape hatch (see `Step` docstring "seam methods ... carry dormant-in-v1 defaults a subclass overrides only when it needs to").
- Returns `PROCEED` when `pr_monitor is None` to preserve the existing test `test_consider_noop_for_review_state_without_pr_review_phase` (no GitHub configured → step is a no-op via `work()` returning CONTINUE).
- Required imports to add to `step.py`: `saga.services.linear.task_state_store as task_state_repo`, `PreAssessment`, `PreDecision` (from `saga.orchestrator.steps.deps`), `TaskState`.

### 3. `src/saga/orchestrator/steps/review/pr_monitor.py` — keep the safety net in `poll()`

Leave the existing `if not prs: prs = await self.recover_prs_from_github(task); if not prs: mark_locally_failed(...)` block in `poll()` (now lines 124–130) **in place** but with two adjustments:

- Update the call to use the renamed `recover_prs_from_github`.
- The `mark_locally_failed("Reached review with no PR to monitor.")` is now defensively unreachable under the new pre_step gate (because `pre_step` would have paused with `NEEDS_INPUT` and routing skips paused tasks). Keep it as a belt-and-braces safety net for the theoretical case where `prs` gets cleared between `pre_step` and `poll()` (e.g. a future code path). Update the log line to call out "safety net" so future readers know it's not the primary defense.

This is deliberate defense-in-depth — removing it would mean a future regression could silently re-introduce the bug.

### 4. `tests/test_orchestrator_pr_review.py` — update + add tests

**Update** the existing test `test_poll_marks_failed_when_no_pr_on_github_and_state_missing`:
- This test currently asserts `pause == Pause.FAILED` when `poll()` is called directly with no PR. That still holds for the safety-net path. Keep the test but reframe its docstring to "the safety net in poll() still fires if pre_step is bypassed (direct poll())". The fix is at the `pre_step` layer, not `poll()`.

**Add** new tests in the existing recovery section:

1. `test_pre_step_pauses_when_no_pr_and_recovery_fails` — `ts.prs=[]`, `branch_name="feature/x"`, `get_pull_by_branch` returns `None` for all repos. Call `PrReviewStep.pre_step(ctx)`. Assert returned `PreAssessment.decision is PreDecision.PAUSE` and the reason mentions the branch name. State should be unchanged (pre_step doesn't persist; the runner does via `pause_for_input`).

2. `test_pre_step_proceeds_when_pr_already_in_state` — `ts.prs=[PRState(...)]`. Call `pre_step`. Assert `PreDecision.PROCEED` and `get_pull_by_branch` was never awaited (skip the recovery round-trip when state already has PRs).

3. `test_pre_step_recovers_pr_and_proceeds` — `ts.prs=[]`, branch set, `get_pull_by_branch` returns a `PullRequest`. Call `pre_step`. Assert `PreDecision.PROCEED`, and `task_states["issue-1"].prs` is now populated by recovery.

4. `test_pre_step_proceeds_when_pr_monitor_is_none` — config with no GitHub. Call `pre_step`. Assert `PreDecision.PROCEED` (so the no-GitHub no-op flow stays intact).

5. **End-to-end** `test_consider_pauses_with_needs_input_when_in_review_without_pr` — drive through `orch.consider(task)` like the existing `test_consider_routes_pr_review_phase_to_maybe_poll_pr` test. `ts.prs=[]`, no PR on GitHub. After awaiting the dispatched agent task, assert `task_states["issue-1"].pause == Pause.NEEDS_INPUT` and `notifier.on_needs_human.assert_awaited_once()`. This is the success-criterion #3 test (a real exercise of the reproduction scenario from FORGE-5, ending in the new defensive behavior).

For tests 1–4, build the `StepCtx` with the same `_ctx(orch, task, cfg, ts)` helper used elsewhere in this file. For test 5, use `_make_orch` + `consider` + `await agent.task` as in the existing consider tests.

## Order of changes

1. Rename `PrMonitor._recover_prs_from_github` → `recover_prs_from_github`; update the single internal caller. Tests still green.
2. Add `pre_step` override on `PrReviewStep` + required imports. Tests still green (no behavior change for happy paths; new gating only fires when `ts.prs=[]` AND recovery fails).
3. Add the 5 new / updated tests in `test_orchestrator_pr_review.py`.
4. Run `just lint && just test`.

## Edge cases & risks

- **Branch name missing.** `recover_prs_from_github` already returns `[]` when `ts.branch_name` is unset (logs a warning). The pre_step pause reason includes `(unset)` so the human sees that the branch wasn't recorded.
- **GitHub transient failure during recovery.** Each repo's `get_pull_by_branch` is wrapped in try/except already — a transient blip in one repo doesn't sink recovery for the others. If *all* repos error, `recovered == []` and we pause; the human reply re-runs `pre_step` which retries. This is exactly the behaviour we want.
- **Multi-repo tickets.** Recovery already iterates `cfg.repos.values()` and accumulates one PR per repo. No change needed for multi-repo.
- **`Pause.STOPPED` / canceled tickets.** The pre_step gate fires inside the runner; the runner is only reached when routing accepted the task, and routing already skips paused/abandoned tasks. The existing `mark_locally_failed` abandoned-state check in `loop.py:546` is unaffected.
- **Resume semantics.** After the pause, a Slack thread reply or human label-removal goes through the existing `consume_thread_reply` / `_unblock` paths (both handle `NEEDS_INPUT`). On resume, `Stage` resets to `ENTERED`, so `pre_step` runs again — recovery is re-attempted, which is the right behaviour if the human opened the PR in the meantime.
- **Subsequent poll ticks.** Once `pre_step` PROCEEDs and `set_stage(WORKING)` runs, subsequent ticks skip `pre_step` and go straight to `work()` (the existing poll loop). So the new gate only adds work on first entry, not on every poll tick.
- **Test `test_poll_marks_failed_when_no_pr_on_github_and_state_missing`** still calls `poll()` directly (bypassing the runner / pre_step). That's the safety-net coverage and is kept intentionally. The new end-to-end test (#5 above) covers the user-facing path through `consider()`.

## Verification

Run from `saga/`:

- `just lint` — ruff check, ruff format check, ty check.
- `just test` — full suite (697 tests today).
- Targeted: `uv run pytest tests/test_orchestrator_pr_review.py -v` (39 existing + 5 new = 44 expected).

**Before-state observation:** `test_poll_marks_failed_when_no_pr_on_github_and_state_missing` passes today (verified) — it documents the current "pause=FAILED, Reached review with no PR to monitor." behavior. This is the bug. The new end-to-end test (#5) is the inverse assertion against the same starting conditions: after the fix, that same scenario lands in `NEEDS_INPUT` with a useful reason.

The project cannot be brought up end-to-end in this worktree (requires `LINEAR_OAUTH_TOKEN`, `GITHUB_APP_*` secrets, a Linear workspace, Slack). The unit tests are the canonical verification per `CLAUDE.md` ("Tests passing IS the verification") and existing FORGE-5-class regressions are all covered by `tests/test_orchestrator_pr_review.py`.

## Out of scope (explicit)

- No change to `implementation/__init__.py` `post_step` — option 3 from the ticket is not pursued.
- No change to how the agent calls `open_pr` (per the ticket's Out of Scope section).
- No change to PR-search logic, only its invocation timing (added `pre_step` call site).
- No artifact captures — this is a pure orchestration/state-machine change with no user-visible interface to screenshot; the test results are the artifact.
