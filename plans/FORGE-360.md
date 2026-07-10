## FORGE-360 — In-Review must stay quiet when nothing changed

### Baseline / Before-state

Environment: Python 3.13 project managed with `uv`. Commands: `just lint`, `just test`, `just lint-fix` (aliased to `uv run ...`). Full existing PR-review test suite (46 tests in `tests/test_orchestrator_pr_review.py`) passes today on this branch.

This ticket is a production bug reproducible only against live Slack/Linear/GitHub (see the two Slack links in the ticket description). I did not attempt to reproduce interactively; the "before" state is established from the ticket's Slack examples ("*No change — same branch (e4ef8d1), no newer CI run (still 29016604054)…*" posted every ~minute) and confirmed by reading `src/saga/orchestrator/steps/review/pr_monitor.py`:

- `PrReviewStep.work` (`review/step.py:26-31`) calls `PrMonitor.poll` unconditionally on every 30-second tick and always returns `CONTINUE`.
- `_poll_inner` (`pr_monitor.py:223-364`) has bounded budgets only for CI (`ci_attempts` vs `github.ci_retry_limit`) and merge conflict (`conflict_attempts` vs `github.conflict_retry_limit`). Comments have **no** budget and no dedup beyond `_is_saga_handled` / `_is_linear_linkback`, so a bot re-posting a new-but-equivalent comment on every CI run dispatches a paid LLM turn on every tick, forever.
- `pr_monitor.py:281` — `if new_comments or human_pushed:` resets both `ci_attempts` and `conflict_attempts` whenever any (post-filter) comment shows up, which lets a comment stream reset the very budgets that would otherwise stop the loop.
- `pr_monitor.py:279` only inspects `mergeable_state == "dirty"` (a real conflict); the `"behind"` state (base advanced, branch protection requires up-to-date branch before merge) is never checked, so a repo that requires up-to-date branches has no automated path to a merge.

The `github.update_branch` REST API is available via `githubkit` (`gh.rest.pulls.async_update_branch`) but is not wrapped in `GitHubClient` yet.

### Scope

This plan adds **two** things and leaves everything else the current code does today alone:

1. A bounded, deterministic **comment retry budget** (`comment_attempts`, capped by `github.comment_retry_limit`, default `3`) mirroring the existing CI / conflict budgets — plus a fix to stop new comments from resetting the CI / conflict budgets (which the ticket description explicitly identifies as a bug that masks those budgets).
2. A **"branch behind base" trigger** for repos whose branch protection requires up-to-date branches, handled **deterministically** (call GitHub's `PUT /repos/{o}/{r}/pulls/{n}/update-branch`) rather than by dispatching an LLM turn. Bounded by `behind_attempts` (default `3`) against `github.behind_retry_limit`.

Out of scope (per the ticket): changing `ci_retry_limit`/`conflict_retry_limit` defaults, changing the dispatched fix-turn prompt in `build_pr_review_prompt`, introducing a new config surface beyond `GitHubCfg` fields, or re-doing the FORGE-101 issue-comments unification work (already fixed in `GitHubClient.list_review_comments`).

### Change list (in dependency order)

#### 1. Extend `GitHubCfg` with two knobs — `src/saga/config.py`

Add to `GitHubCfg` (`config.py:18-20`):
```python
class GitHubCfg(StrictModel):
    ci_retry_limit: int = 3
    conflict_retry_limit: int = 3
    comment_retry_limit: int = 3    # NEW
    behind_retry_limit: int = 3     # NEW
```
`StrictModel` uses `extra="forbid"`, so existing YAML without these keys keeps working (fields have defaults). No new config file surface — this is a knob only, not a required setting.

#### 2. Extend `PRState` with two counters — `src/saga/schemas/state.py`

Add to `PRState` (`state.py:61-76`):
```python
class PRState(BaseModel):
    repo: str
    number: int
    review_watermark: datetime | None = None
    last_head_sha: str | None = None
    ci_attempts: int = 0
    conflict_attempts: int = 0
    last_conflict_sha: str | None = None
    comment_attempts: int = 0        # NEW
    behind_attempts: int = 0         # NEW
```
Update `PRState.reset_retries` to zero the two new counters as well — it's the "resume from human unblock" reset used by `_unblock` (`loop.py:378`) and by the HTTP `approve` handler (`entrypoints/http/server.py:241`). Both call sites are unchanged.

`TaskState._migrate_legacy` (`state.py:295-390`) doesn't need to touch these fields — pydantic reads the defaults for absent keys — so already-persisted comments continue to load cleanly.

#### 3. Add `update_branch` wrapper on `GitHubClient` — `src/saga/services/github/client.py`

Add a minimal wrapper around `gh.rest.pulls.async_update_branch`, alongside the existing PR helpers (`client.py:525-598`):
```python
async def update_branch(self, owner_repo: str, number: int) -> bool:
    """PUT /pulls/{n}/update-branch — merge base into the PR branch server-side.

    Returns True on 202 (queued), False if GitHub rejected the update (e.g. a real
    conflict). GitHub processes the update asynchronously — the new head SHA appears
    on the next get_pull call, so we don't return it here.
    """
    owner, repo = self._split(owner_repo)
    try:
        await self._gh.rest.pulls.async_update_branch(owner=owner, repo=repo, pull_number=number)
        return True
    except RequestFailed as exc:
        # 422 = merge conflict; anything else is transient/permission — log and let the
        # caller retry next tick (bounded by behind_attempts).
        logger.warning(
            f"update_branch failed repo={owner_repo} number={number} status={exc.response.status_code}"
        )
        return False
```
No new import — `RequestFailed` is already imported at the top of `client.py`.

Rationale for "returns bool, no SHA": GitHub's endpoint is asynchronous and 202-returns; the resulting merge-commit SHA is only observable from a subsequent `get_pull` call. Keeping the wrapper narrow avoids racing GitHub's own state.

#### 4. Fix `_poll_inner` — `src/saga/orchestrator/steps/review/pr_monitor.py`

This is the heart of the change. The rewrite preserves every existing behavior test (see §Testing) and adds four things: the comment budget, the CI/conflict-reset scope fix, the "behind" trigger, and its own budget.

Concrete diff, keeping the line-anchored structure of the file:

**a. Extend the tick-start reset (line 281) to only reset on `human_pushed`, and to also reset the two new counters when a push happens.** Comments no longer reset CI / conflict — the ticket description calls this out explicitly as a bug ("*a comment stream can also mask/restart the CI/conflict budgets that would otherwise have stopped the loop*"). A human push is a real signal that the underlying state changed; a new comment is not.

Replace lines 281-292:
```python
if human_pushed:
    pr_state = pr_state.model_copy(
        update={
            "ci_attempts": 0,
            "conflict_attempts": 0,
            "last_conflict_sha": None,
            "comment_attempts": 0,
            "behind_attempts": 0,
        }
    )
    await self._save_pr(task.id, pr_state)
    ci_attempts = 0
    conflict_attempts = 0
    conflict_sha = None
    comment_attempts = 0
    behind_attempts = 0
else:
    ci_attempts = pr_state.ci_attempts
    conflict_attempts = pr_state.conflict_attempts
    conflict_sha = pr_state.last_conflict_sha
    comment_attempts = pr_state.comment_attempts
    behind_attempts = pr_state.behind_attempts
```

**b. Add the "behind" trigger next to the "conflict" and "failing CI" triggers (lines 275-297).** Compute it from the same `mergeability_known` gate the conflict detection uses (so we don't act on an unknown state), and only when there's no conflict (a `dirty` state supersedes `behind` in GitHub's own enum but be defensive):
```python
behind = mergeability_known and pr.mergeable_state == "behind" and not conflict
```

**c. Wire the new triggers into the budget-check + dispatch block (lines 299-360).** The comment budget is bookended around the existing comment path; the `behind` handler is a separate deterministic branch that runs BEFORE the LLM-dispatch block:

```python
ci_limit = self._cfg.github.ci_retry_limit if self._cfg.github else 3
conflict_limit = self._cfg.github.conflict_retry_limit if self._cfg.github else 3
comment_limit = self._cfg.github.comment_retry_limit if self._cfg.github else 3
behind_limit = self._cfg.github.behind_retry_limit if self._cfg.github else 3

has_comments = bool(new_comments)
has_conflict = conflict and conflict_sha != pr.head_sha
has_failing_ci = bool(failing)

# Budget guards — same shape as CI / conflict today.
if has_failing_ci and ci_attempts >= ci_limit:
    # ... existing code unchanged
if has_conflict and conflict_attempts >= conflict_limit:
    # ... existing code unchanged
if has_comments and comment_attempts >= comment_limit:
    logger.warning(f"comment retry budget exhausted task={task.id}")
    await self._record_friction(
        task.id, FailureClass.COMMENT_LOOP, None, comment_attempts
    )
    await self._mark_locally_failed(
        task.id,
        f"Comment retry budget exhausted for PR #{pr_state.number} — the same review"
        f" feedback has been re-triggered {comment_attempts} times without human progress.",
    )
    return

# Deterministic "branch behind" handling — no LLM turn, no Slack message.
if behind:
    if behind_attempts >= behind_limit:
        logger.warning(f"branch-behind retry budget exhausted task={task.id}")
        await self._record_friction(
            task.id, FailureClass.BRANCH_BEHIND, pr.head_sha, behind_attempts
        )
        await self._mark_locally_failed(
            task.id,
            f"Unable to keep PR #{pr_state.number} up to date with base after"
            f" {behind_attempts} attempts (GitHub update-branch kept failing).",
        )
        return
    ok = await self._github.update_branch(pr_state.repo, pr_state.number)
    new_behind = behind_attempts + 1 if not ok else 0
    await self._save_pr(
        task.id, pr_state.model_copy(update={"behind_attempts": new_behind})
    )
    # Don't dispatch: next poll re-checks. On success, head advances and mergeable_state
    # flips to clean/unstable. On repeated failure, this branch escalates via the budget above.
    return

if has_comments or has_conflict or has_failing_ci:
    trigger_parts = []
    if has_comments:
        trigger_parts.append("comments")
    if has_conflict:
        trigger_parts.append("conflict")
    if has_failing_ci:
        trigger_parts.append("ci")

    pr_updates: dict[str, Any] = {}
    if has_failing_ci:
        pr_updates["ci_attempts"] = ci_attempts + 1
    if has_conflict:
        pr_updates["conflict_attempts"] = conflict_attempts + 1
        pr_updates["last_conflict_sha"] = pr.head_sha
    if has_comments:
        pr_updates["comment_attempts"] = comment_attempts + 1   # NEW
    if pr_updates:
        await self._save_pr(task.id, pr_state.model_copy(update=pr_updates))

    # (existing friction records for CI + conflict stay unchanged)

    await self._dispatch_turn(...)  # unchanged call
elif not mergeability_known:
    pass
else:
    await self._save_pr(task.id, pr_state.model_copy(update={"last_head_sha": pr.head_sha}))
```

Order matters: the `behind` branch is `return`-ing before `_dispatch_turn`, so a `behind` PR never spends LLM tokens. If a PR is *simultaneously* behind and has new comments/CI/conflict, we prefer the deterministic branch-update this tick and address the other signals next tick (after `update-branch` completes). This slightly stretches recovery but keeps each tick single-purpose and cheap.

#### 5. Add two new `FailureClass` values — `src/saga/schemas/state.py`

`FailureClass` (`state.py:86-95`) needs new labels for the two new escalation reasons so terminal aggregation attributes them correctly:
```python
COMMENT_LOOP = "comment_loop"      # NEW — comment retry budget exhausted
BRANCH_BEHIND = "branch_behind"    # NEW — update-branch retry budget exhausted
```
Terminal aggregation (`orchestrator/steps/terminal/aggregate.py`) currently reads `failure_class.value` opaquely into the summary — no separate mapping needed. Grep for `FailureClass` usages to confirm no exhaustive-match on the enum before shipping.

### Testing plan

Add to `tests/test_orchestrator_pr_review.py`, using the existing `_pr`, `_comment`, `_check_run`, `_make_orch` fixtures. Fake the notifier and session as those tests already do — the assertion "no tokens spent, no Slack message" is *mocks receive zero calls*.

**New tests — comment budget:**

1. `test_poll_comment_retry_budget_exhausted_marks_failed` — start with `PRState(comment_attempts=3)` plus a fresh actionable comment. Assert `pause == Pause.FAILED`, `_dispatch_turn` never called, a `COMMENT_LOOP` StepRecord appears.
2. `test_poll_comment_dispatch_increments_counter` — start with `comment_attempts=0`, one comment. Assert `_dispatch_turn` called once, `comment_attempts == 1` after.
3. `test_poll_repeated_bot_comment_converges_to_failed` — simulate three consecutive ticks with a fresh (different id / timestamp) actionable bot comment on each tick; assert exactly three dispatches then one `mark_locally_failed`. This is the concrete "unbounded → bounded" regression the ticket asks for.
4. `test_poll_new_push_resets_comment_attempts` — start with `comment_attempts=2`, PR head advances (`human_pushed=True`); assert `comment_attempts == 0` after.

**New tests — "branch behind" trigger:**

5. `test_poll_behind_calls_update_branch_no_dispatch` — `mergeable_state="behind"`; assert `github.update_branch` called once, `_dispatch_turn` NOT called, notifier NOT called, session send NOT called.
6. `test_poll_behind_success_next_tick_is_noop` — first tick: behind → update_branch(True). Second tick with `mergeable_state="clean"`: no dispatch, `behind_attempts` reset to 0.
7. `test_poll_behind_update_branch_failure_increments_and_eventually_fails` — three consecutive ticks where `update_branch` returns False; assert `behind_attempts` grows 1→2→3, fourth tick with `behind_attempts=3` marks `Pause.FAILED` with `FailureClass.BRANCH_BEHIND`.
8. `test_poll_dirty_takes_precedence_over_behind` — PR returns `mergeable_state="dirty"` (conflict). Assert existing conflict flow runs (dispatches turn), `update_branch` NOT called.

**New test — the headline "quiet when nothing changes" assertion:**

9. `test_poll_no_change_across_ten_ticks_makes_zero_llm_or_slack_calls` — set up a stable PR (clean, no comments, no failing CI, mergeable), then call `poll()` ten times. Assert: `_dispatch_turn` never called, notifier `post_agent_message` / `on_needs_human` never called, session `send` never called. This is the ticket's headline success criterion.

**Fix to existing test — CI/conflict reset scope:**

10. Update `test_poll_new_push_resets_conflict_attempts` (line 901) to also confirm `comment_attempts` and `behind_attempts` are zeroed on push. No other existing tests assert on the "new comment resets ci/conflict" behavior (grep confirmed), so removing that reset does not break the current suite. If any hidden test does rely on it, decide case-by-case — my read is that reset-on-comment was structural, not intentional, so tests should be updated to match the new semantic.

**Regression coverage (must remain green):**

- `test_poll_no_signals_updates_head_sha`
- `test_poll_new_comment_dispatches_review_turn`
- `test_poll_failing_ci_dispatches_and_increments_counter`
- `test_poll_ci_retry_budget_exhausted_marks_failed`
- `test_poll_conflict_dispatches_review_turn`
- `test_poll_conflict_already_dispatched_for_sha_is_noop`
- `test_poll_conflict_retry_budget_exhausted_marks_failed`
- `test_poll_mergeable_unknown_skips_without_updating_head_sha`
- `test_poll_saga_handled_reply_skips_dispatch_and_advances_watermark`
- `test_poll_human_reply_after_saga_fixed_dispatches_turn`
- `test_poll_watermark_covers_filtered_saga_reply`
- `test_poll_linear_linkback_skips_dispatch_and_advances_watermark`
- `test_poll_human_comment_after_linkback_still_dispatches`
- `test_poll_comments_and_ci_failure_dispatches_single_turn`
- `test_poll_transient_error_does_not_escalate`
- All the `_dispatch_turn` / recovery / stale-session tests

### Verification commands (from repo root)

```bash
uv run pytest tests/test_orchestrator_pr_review.py   # focused
uv run pytest                                         # full suite before shipping
uv run ruff check && uv run ruff format --check && uv run ty check
```

### Edge cases + risks

- **Filtered-but-present comments** (Saga-handled / Linear linkback). Today's line 252-259 short-circuits and advances the watermark before any budget code — that path is unchanged by this plan, so the comment budget never increments for a filtered-out tick. Verified in `test_poll_saga_handled_reply_skips_dispatch_and_advances_watermark`.
- **`update_branch` needs `contents: write` on the repo.** Saga's GitHub App already has this (it's what `publish_worktree` uses to push replay commits), so no new permission is required — but note this in the PR description for the reviewer.
- **`mergeable_state == "behind"` semantics.** GitHub only returns `"behind"` when the *branch protection setting* "Require branches to be up to date before merging" is enabled on the base branch; if not enabled, being behind base returns `"clean"` (or `"unstable"` if there are failing non-required checks). So there's no need for us to read branch-protection settings separately — GitHub's own enum already encodes the "only if the repo requires it" gate from the ticket.
- **Async `update_branch`.** GitHub 202-queues the update; the new head SHA appears on a later `get_pull`. Keep `last_head_sha` untouched in the behind branch — the next tick's `human_pushed` compare will notice the change and reset the counters naturally.
- **Interaction with the fix-turn agent's own comment replies.** The dispatched agent posts `<!-- saga-comment --><!-- saga-fixed -->` replies, which the existing filter treats as handled — so those replies don't feed the comment budget. Confirmed by re-reading `_is_saga_handled` at line 42.
- **Interaction with `_unblock` on human approve.** `reset_retries` (`state.py:72`) also zeroes the new counters, so the human "Approve"/"retry" path gives the same fresh budget on the new counters that it already gives on `ci_attempts` / `conflict_attempts`.

### Definition of done

- All existing PR-review tests still pass; the ten new tests above pass.
- `just lint` and `just test` are clean.
- The four wake-up conditions the ticket names — unanswered comments, failing CI, merge conflicts, branch behind (where required) — are each detected and each **bounded**. When none of them apply, `_poll_inner` returns without any call to `_dispatch_turn`, `SlackNotifier`, or `session.send` — proven by `test_poll_no_change_across_ten_ticks_makes_zero_llm_or_slack_calls`.
- A recurring bot comment converges to a single `Pause.FAILED` escalation instead of infinite turns — proven by `test_poll_repeated_bot_comment_converges_to_failed`.