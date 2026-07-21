# FORGE-495 — pr_review fix turns silently fail to publish local commits

## Root cause

A `pr_review` fix turn runs in a worktree that is **never deterministically positioned on the PR branch at the remote PR head**, yet `publish_worktree` computes its diff base as that remote PR head. The two disagree, and the resulting no-op is reported as success.

Trace (`pr_monitor.py:_dispatch_turn`, 474-631):
1. `create_workspace(task.id, repos=triaged)` (`workspace.py:440`) — reuses an existing worktree **as-is**, or, if it was cleaned/lost (orchestrator restart + stale-cleanup race, disk cleanup, or a triaged clone that never had the branch), creates a **fresh worktree detached at the base branch** (`worktree add --detach origin/<base>`), i.e. on `main`, not the PR branch.
2. `update_workspace(task.id)` (`workspace.py:479`) — for a detached worktree, re-detaches it to the **base branch** tip; for a branch worktree it skips. Neither path ever checks out the PR branch. It is actively wrong for pr_review (pulls the tree back to `main`).
3. The prompt (`prompts.py:328-334`) then tells the *agent* to "Fetch the latest changes from the PR branch yourself … commit the intended changes locally." So correct branch positioning is delegated to the LLM.
4. The session is **resumed** across turns (`get_or_open` with `session_id`). A resumed agent that "remembers" committing a fix on an earlier turn will not re-fetch / re-checkout / re-commit — but the physical worktree (detached at base, or reset to remote head after a prior publish) does **not** contain that commit.
5. `_publish_pr_changes` → `publish_worktree` (`client.py:382`): `base_sha = ensure_branch(PR branch)` = **remote PR head**; `_local_commits(repo_dir, base_sha)` = `git rev-list base_sha..HEAD`. When the worktree HEAD is not a descendant-with-new-commits of the remote PR head (agent committed on top of `main`, left changes uncommitted, or committed nothing), this is empty → `publish_worktree` returns `None`.
6. Back in `_dispatch_turn` (596-606), `published_sha is None` is treated as **success**: it just sets `last_head_sha` to the *unchanged* remote head and returns. No `mark_locally_failed`, no friction record. From the outside: "ran the agent, nothing changed."

Real GitHub API failures during publish already raise → caught by the outer `except Exception` (608) → `mark_locally_failed`. The gap is **only** the silent `None` (nothing-to-push) path plus the fragile worktree positioning that produces it.

## Fixes (in dependency order)

### 1. Deterministically place the fix-turn worktree on the PR branch at the remote head
Add a `WorkspaceManager` method (e.g. `sync_to_remote_branch(task_id, repo_key, branch_name)`) in `src/saga/services/git/workspace.py`:
- Resolve `target = workspace_path/repo_key`, `bare = bare_dir(...)`; best-effort skip (log) if either is missing, matching sibling methods.
- `await fetch_branch_ref(bare, branch_name)` (fetches `+refs/heads/<branch>:refs/remotes/origin/<branch>`).
- `await run_git(target, ["checkout", "-B", branch_name, origin_ref(branch_name)])` then `["reset", "--hard", origin_ref(branch_name)]`.
- This leaves the worktree on `branch_name` at the exact SHA `publish_worktree` will use as `base_sha`, so any commit the agent makes is reliably `base_sha..HEAD`. It also self-heals the resumed-session case: the agent now sees the true branch state (its supposed prior commit is absent) and redoes it.

In `pr_monitor.py:_dispatch_turn` (around 499-500): after `create_workspace`, **replace the `update_workspace(task.id)` call** (which wrongly detaches to base) with a call to the new method for the PR's repo. `branch_name` comes from task state (as `_publish_pr_changes` already reads it, 620); `pr_repo_key` is already computed at 494-497. If `branch_name` is missing, keep current behavior (return/mark failed as today).

Tradeoff (call out, acceptable): resetting the worktree to the remote PR head discards any *unpublished* local commit from a prior turn. Those are exactly the phantom/unreliable commits this bug is about; discarding and letting the agent rebuild from the known-good remote head is the safe, self-healing choice. It is a local reset only — no rebase, no force-push — consistent with `.claude/rules/git-workflow.md`. (`publish_worktree`'s own post-publish `_sync_worktree_to_remote` already does the same `checkout -B / reset --hard` pattern.)

### 2. Surface the silent no-op instead of reporting success (success criterion #3)
In `_dispatch_turn`, after a non-failed turn, distinguish "published a new commit" from "nothing to publish". Have `_publish_pr_changes` report enough to decide (e.g. return the `PublishedCommit | None` it already gets, plus optionally whether the worktree had uncommitted changes via `git status --porcelain` / local-commit count on the PR repo dir).
- When the trigger required a code change (`conflict` and/or `ci`) **and** publish returned `None` **and** the remote head did not advance vs `pr_state.last_head_sha`: call `mark_locally_failed` with a diagnosable message, e.g. *"PR review fix turn (trigger=conflict) completed but produced no commit to publish; PR branch head unchanged at <sha>. Agent may have left changes uncommitted or believed a prior commit was already published."* Include a `_record_friction` entry.
- For **comment-only** triggers, keep the current no-op-is-fine behavior (the agent may legitimately just reply); the existing `comment_attempts` budget already bounds runaway dispatch (FORGE-360). Do not fail these.
- Optional but recommended diagnostic: if the worktree is dirty (uncommitted changes) after any trigger's turn, log a WARNING and include it in the friction summary — this is the exact "agent forgot to commit" signature from the incident.

## Edge cases / risks
- **PR branch not yet on remote**: not applicable at pr_review — `open_pr` (`mcp_tools.py:293`) already published it. If `fetch_branch_ref` fails, method logs and skips; publish then behaves as today.
- **Multi-repo tickets**: only the PR's own repo needs syncing; iterate as `_publish_pr_changes` does (match `repo_cfg.github == pr.repo`). Triage-narrowed repo sets already force-include the PR repo (494-498) — keep that.
- **Human-pushed / behind base**: syncing to `origin/<branch>` picks up the human's commits, which is strictly better than the old detach-to-base. Budget-reset logic (293-308) is unaffected.
- **False-positive failures**: scoping the new `mark_locally_failed` to ci/conflict triggers avoids failing legitimate comment-reply turns.
- **Do not touch** trigger detection, retry budgets, or the FORGE-463 `ty` migration content (explicitly out of scope).

## Files
- `src/saga/services/git/workspace.py` — new `sync_to_remote_branch` method.
- `src/saga/orchestrator/steps/review/pr_monitor.py` — swap `update_workspace` for the new sync in `_dispatch_turn`; add no-op-publish detection + surfacing.
- `src/saga/orchestrator/steps/review/pr_monitor.py` (or `_publish_pr_changes`) — return richer publish result if needed.
- Tests: `tests/test_workspace.py`, `tests/test_orchestrator_pr_review.py`, and optionally `tests/test_github_client.py`.

## Tests (regression — success criteria #1/#2/#3)
1. **`tests/test_workspace.py`**: set up a bare "remote" with base `main` at M and PR branch `feature` at P (=M+PR commits). Create a task worktree detached at M (the fresh-clone/reset case). Call `sync_to_remote_branch(task, repo_key, "feature")`; assert the worktree is on branch `feature` at SHA P (`rev-parse HEAD` == P, `rev-parse --abbrev-ref HEAD` == `feature`). Then simulate an agent commit and assert `_local_commits(repo_dir, P)` (or `git rev-list P..HEAD`) contains it — i.e. the commit is now detectable/publishable, whereas without the sync (worktree at M) it is not `base_sha..HEAD`-related. This is the "agent commits locally, publish reports no local commits → now detected" reproduction.
2. **`tests/test_orchestrator_pr_review.py`**: (a) assert `_dispatch_turn` calls the new sync method (add it to `_FakeWorkspace`, lines 152-165 and the near-1866/1961 fakes) with the PR branch; (b) new test: conflict/ci trigger + `publish_worktree` returning `None` + `get_pull` head unchanged → asserts `mark_locally_failed` was called (not silent success); (c) comment-only trigger + `None` publish → asserts NOT failed (preserve current behavior).

## Verify
- `just test` (or scoped: `uv run pytest tests/test_workspace.py tests/test_orchestrator_pr_review.py tests/test_github_client.py -q`).
- `just lint` (ruff + `ty`).
- Baseline confirmed in this sandbox: `uv run pytest tests/test_orchestrator_pr_review.py -q` → 53 passed. New regression test must fail before fix #1/#2 and pass after.
- **Before**: worktree not on PR branch (or resumed agent doesn't re-commit) → `publish_worktree` returns `None` → `last_head_sha` set to unchanged remote head → turn "succeeds", PR head frozen (the `cx-olly#544` "still identical" pattern).
- **After**: fix-turn worktree is on the PR branch at the remote head → agent's commit lands in `base_sha..HEAD` → published, PR head advances; and a genuine no-commit on a ci/conflict turn is surfaced via `mark_locally_failed`/friction instead of silent success.
- **Note**: live repro against `coralogix/cx-olly#544` is not possible in this sandbox (no GitHub App credentials / no network to the real PR). The before-state is captured by the deterministic git-level regression test above, per the ticket's own success-criteria framing.