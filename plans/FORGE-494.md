# FORGE-494 — Publish diverged-remote reconciliation + gated "fixed" claim

## Root cause (confirmed)

Saga never `git push`es; `PrMonitor._dispatch_turn` runs the fix turn, then `_publish_pr_changes` → `GitHubClient.publish_worktree` replays local commits as App-verified commits and `update_ref`s the branch (`client.py:382-512`).

The fix-turn worktree can carry an **older base** than the live remote PR tip:
- `create_workspace` reuses an existing worktree without resetting it, and `update_workspace` (`workspace.py:479-513`) skips any worktree that is on a branch (which `_sync_worktree_to_remote`'s `checkout -B branch` leaves it on). So HEAD stays at the last-published tip `P`.
- If a human clicks GitHub's "Update branch" / auto-merge, the remote PR tip advances to `R = P + "Merge branch 'master'…"` that the worktree never pulls.

Then in `publish_worktree`: `base_sha = R` (live API tip), `_fetch_branch` fetches `R`, and `_local_commits = git rev-list --first-parent R..HEAD` returns the fix commits whose first-parent is `P` (an ancestor of `R`, not `R`). The replayed tip descends from `P`, **not** from `R`, so the final `async_update_ref(force=False)` is a non-fast-forward → GitHub 422, OR (when HEAD is already an ancestor of `R`) `_local_commits` is empty → `return None`. Either way the fix does not land. This is exactly FORGE-22 (canceled unfixed) recurring.

Verified the git mechanics locally: with local HEAD = base+fix and remote tip `R` = base+master-merge, `R` is not an ancestor of HEAD; `git merge R` into HEAD makes `R` an ancestor again and `rev-list R..HEAD` then yields `[fix, merge]` — which the existing merge-replay path (see `test_publish_worktree_merge_of_origin_main`) already handles, producing a tip that descends from `R` and updates cleanly.

## Run / check commands (from justfile + CLAUDE.md)
- Tests: `uv run pytest tests/test_github_client.py tests/test_orchestrator_pr_review.py -q` (scope to changed modules).
- Lint/types: `just lint` (`ruff check`, `ruff format --check`, `ty check`).
- Baseline confirmed green: `tests/test_github_client.py` 21 passed. This bug has no runnable repro without a live GitHub App; the regression tests below ARE the before/after evidence (new tests fail on current code, pass after the fix).

## Changes (in dependency order)

### 1. `src/saga/services/github/client.py` — reconcile a diverged remote before replaying
In `publish_worktree`, after `await _fetch_branch(...)` and computing `base_sha`, and **before** `_local_commits`:
- Check ancestry: run `git merge-base --is-ancestor <base_sha> HEAD` in `repo_dir` (add a small `_is_ancestor(repo_dir, ancestor, descendant) -> bool` helper using `_run_git`, returning False on the non-zero exit rather than raising).
- If `base_sha` is **not** an ancestor of HEAD (diverged), perform a merge-only reconciliation in the worktree:
  - `git merge <base_sha> --no-edit -m "saga: merge remote tip <base_sha[:12]> before publish"`.
  - Respect the merge-only git rule (`.claude/rules/git-workflow.md`): merge only, never rebase/force.
  - If the merge exits non-zero (real conflict this turn's agent didn't resolve), run `git merge --abort` (best-effort) and `raise GitPublishError(...)` describing the unreconcilable divergence. This propagates to `_dispatch_turn`'s outer `except` → `needs-human`.
- Then proceed unchanged: `_local_commits(repo_dir, base_sha)` now includes the fix commits plus the reconciling merge commit; the existing replay loop maps `base_sha → base_sha`, resolves the pre-base first-parent via `async_get_commit`, and passes the merge's second parent (`base_sha`) through — the final replayed tip descends from `base_sha`, so `update_ref(force=False)` succeeds.
- Do **not** weaken `force=False`. The non-fast-forward guard remains the safety net; reconciliation is what makes the update legitimately fast-forwardable.

Note: the genuine "no local commits" case still returns `None` (agent replied without a code change) — that stays legitimate. The unreconcilable case now consistently **raises** rather than silently returning `None`.

### 2. `src/saga/orchestrator/steps/review/pr_monitor.py` — gate success on a verified remote-ref advance
In `_dispatch_turn`, after `published_sha = await self._publish_pr_changes(...)` (line ~596):
- Keep the existing raise→`needs-human` path (outer `except` at line 608 already calls `_mark_locally_failed`).
- Add: when the turn was triggered by **CI failure or merge-conflict** (`failing` non-empty or `conflict is True`) and `published_sha is None` (nothing landed on the remote ref), treat it as a failed publish:
  - `await self._record_friction(task.id, FailureClass.PUBLISH_FAILED, f"publish produced no remote-ref advance (trigger)", attempts)`.
  - `await self._session_mgr.close(task.id)` then `await self._mark_locally_failed(task.id, "PR review fix did not land on the remote branch (no publishable commit) — <detail>.")` and `return` **before** advancing `review_watermark` / `last_head_sha`, so the unaddressed comment/CI is retried after a human unblocks.
  - **Decision (asked, unanswered — chose lowest false-positive default):** comment-only turns that legitimately produce no commit (a question/answer reply) do **not** escalate. Only CI/conflict triggers, which always require a code change, escalate on a `None` publish. Easy to tighten to "any no-advance turn" in review if desired.
- When `published_sha is not None`, behavior is unchanged (ref advanced → success, watermark advances).

### 3. `src/saga/schemas/state.py` — add `FailureClass.PUBLISH_FAILED`
Add `PUBLISH_FAILED = "publish_failed"  # publish_worktree landed no commit on the remote ref` to the `FailureClass` enum (`state.py:94`) for a diagnosable friction record.

### 4. `src/saga/services/claude/prompts.py` — minor reinforcement (low priority)
The `<!-- saga-fixed -->` reply is posted by the agent mid-turn, before publish, so it cannot be gated on the publish result directly — the real gate is Saga-side (steps 1–2). Optionally tighten the guidance around lines 271-277 / 328-334 to say Saga verifies the publish landed on the remote branch and will re-open the thread (needs-human) if it did not, so a "fixed" reply is not the source of truth. No behavioral dependency; keep minimal.

## Tests

### `tests/test_github_client.py` — regression for divergence reconciliation
New `test_publish_worktree_reconciles_diverged_remote` modeled on `test_publish_worktree_merge_of_origin_main` (uses real `_setup_repo`/`_git`, mocks only the githubkit REST surface):
- Set up remote `main`, push `base`. Local branch commits a fix on `base` (HEAD = base→fix). Separately create the remote PR-tip `R` = base + a "Merge branch master" commit **not** in local HEAD; make `async_get_ref` return `R` and `async_get_commit` resolve `R`, `base`, and the pre-base parent trees.
- Assert: `publish_worktree` performs the reconciling merge, replays fix + merge, and the final `async_update_ref` `data["sha"]` is the replayed tip that has `R` in its parent chain (i.e. `force=False` update is valid). Assert result is non-`None`.
- Add `test_publish_worktree_unreconcilable_divergence_raises`: a merge that conflicts (or a 404 pre-base parent) → `publish_worktree` raises `GitPublishError` (not `None`).

### `tests/test_orchestrator_pr_review.py` — escalation on failed publish
- `test_pr_review_publish_none_on_ci_trigger_marks_needs_human`: drive a `_dispatch_turn` with a CI-failure (or conflict) trigger where `_publish_pr_changes`/`publish_worktree` returns `None`; assert the task ends `Pause.FAILED` (via the injected `mark_locally_failed` fake) and that `review_watermark`/`last_head_sha` were **not** advanced.
- `test_pr_review_publish_raises_marks_needs_human`: `publish_worktree` raises → `Pause.FAILED` (guards the existing outer-except path).
- Follow existing fakes/fixtures in the file (mock GitHub, Slack, session; use the `task_states` fixture); mock at boundaries only.

## Edge cases / risks
- **Merge conflict during reconciliation**: real conflict the agent didn't resolve → raise → needs-human (correct; a human/next turn resolves). Must `git merge --abort` first so the worktree isn't left mid-merge.
- **Pre-base first-parent**: the existing 404-vs-transient handling (`client.py:437-446`) is preserved; a transient 5xx/403/429 still propagates with its real type so the caller can retry, not a false "unexpected ancestry".
- **Over-escalation**: bounded to CI/conflict triggers (see decision above) to avoid flagging legitimate comment-only replies.
- **`force=False` invariant** and the merge-only rule are both preserved — no rebase, no force-push.
- **FailureClass addition**: verify no exhaustiveness/match on `FailureClass` elsewhere needs a new arm (grep usages before finalizing).

## Verify
- `uv run pytest tests/test_github_client.py tests/test_orchestrator_pr_review.py -q` — new tests fail on current code, pass after changes; existing 21 github tests stay green.
- `just lint` clean.
- Manual (per ticket): on a PR whose base was merged into the head branch via GitHub after the worktree last fetched, a subsequent fix turn's commit appears on the PR remote branch — `gh api repos/<repo>/pulls/<n>` `head.sha` advances; and a forced-unreconcilable case lands the ticket in `needs-human` instead of going quiet.