# FORGE-18 — Implementation falsely reports no PR opened

## Summary

The `implementation` post-step guard at `saga/src/saga/orchestrator/steps/implementation/__init__.py:293-303` is meant to catch agents that finish without calling `open_pr`. It fires false-positives: it sees `ts.prs == []` even after `open_pr` ran successfully (proven because the PR was attached to the Linear ticket — `create_attachment` runs strictly after `update_state(prs=...)` inside `open_pr`).

Two compounding issues make this possible. We fix the underlying state-write race and add a deterministic GitHub-side recovery so the guard can only fire when there genuinely is no PR.

## Root-cause analysis

### Issue 1 — TOCTOU in `open_pr`'s state write (real, easy to fix)

`saga/src/saga/services/claude/mcp_tools.py:255-257`:

```python
ts = await task_state_repo.get(task_id) or TaskState()          # ← read OUTSIDE the per-task lock
new_pr = PRState(repo=repo, number=pr.number, last_head_sha=publish_sha or pr.head_sha)
await task_state_repo.update_state(task_id, prs=[*ts.prs, new_pr])
```

`update_state` itself is locked, but the `[*ts.prs, new_pr]` list is built from a **lock-free** snapshot of `ts`. `_merge` (`task_state_store.py:124-128`) then **replaces** the `prs` field with whatever value was passed in — it does not concatenate. So any concurrent write to `prs` between the unprotected read and the locked write is silently lost. This is a real lost-update race; how often it manifests depends on what else writes `prs`, but it is wrong regardless.

Contrast with `append_step_record` (`task_state_store.py:189-198`), which reads + appends + writes all inside the lock — the correct pattern.

### Issue 2 — `_fetch` is search-filter-backed and likely eventually consistent (suspected, mitigated)

`task_state_store._fetch` looks up the state comment via `comments(filter: { body: { contains: _MARKER }, issue: { id: { eq: task_id } } })` — a body-text search that on most backends rides a search index. When `open_pr` mutates the comment body via `commentUpdate` and `post_step` immediately re-reads via the same `contains` filter, the search index can briefly return the pre-update body (or fail to match). That returns `None`, `get` returns an empty `TaskState`, the guard sees `not ts.prs`, and the false-positive fires. The post_step sequencing makes this even more likely: `update_state(session_id=...)` at line 262 already re-fetches with the same filter, so this is the **second** filtered read within roughly a second of the write.

We can't deterministically prove this is the cause from inside the worktree, but it is consistent with the symptom ("PR attached to Linear, so the write happened, but the next read missed it"). The fix below makes it irrelevant: when state-says-no-PR, we ask GitHub directly before failing.

## Changes

### 1. Add an atomic `append_pr` to the state store

**File:** `saga/src/saga/services/linear/task_state_store.py`

Add a public helper modeled exactly on `append_step_record`:

```python
async def append_pr(task_id: str, pr: PRState) -> None:
    """Atomically add (or replace, by repo) a PR in the task's prs list.

    Read-modify-write under the per-task lock — eliminates the TOCTOU race that
    existed when callers built `prs=[*ts.prs, new_pr]` from a lock-free snapshot
    and then called update_state, which overwrites rather than merges lists.
    """
    logger.debug(f"append_pr task={task_id} repo={pr.repo} number={pr.number}")
    lock = await _lock_for(task_id)
    async with lock:
        comment_id, current = await _fetch_or_new(task_id)
        # Replace existing PR for the same repo (idempotent), append otherwise.
        merged_prs = [p for p in current.prs if p.repo != pr.repo] + [pr]
        merged = _merge(current, {"prs": merged_prs})
        await _write(comment_id, task_id, merged)
```

Add the import for `PRState` at the top (already present transitively through `TaskState`, but make the direct import explicit for clarity).

### 2. Use `append_pr` in `open_pr`

**File:** `saga/src/saga/services/claude/mcp_tools.py` (lines 255-260)

Replace:

```python
ts = await task_state_repo.get(task_id) or TaskState()
new_pr = PRState(repo=repo, number=pr.number, last_head_sha=publish_sha or pr.head_sha)
await task_state_repo.update_state(task_id, prs=[*ts.prs, new_pr])

# Re-read state after writing the PR so we have the latest notifier handle.
ts = await task_state_repo.get(task_id) or TaskState()
```

with:

```python
new_pr = PRState(repo=repo, number=pr.number, last_head_sha=publish_sha or pr.head_sha)
await task_state_repo.append_pr(task_id, new_pr)

# Re-read state after writing the PR so we have the latest notifier handle.
ts = await task_state_repo.get(task_id) or TaskState()
```

The lock-free `ts = await ... get` at line 255 is no longer needed; `append_pr` does its own locked read. Keep the second re-read (line 260) — the notifier path below depends on it and `append_pr` doesn't return the merged state.

Note: the earlier read at line 174 (idempotency check `existing = ts.pr_for(repo)`) stays as-is — that's a cache-hit short-circuit, and `append_pr`'s "replace by repo" semantics make a stale read there harmless (a concurrent write of the same PR is overwritten with the same data).

### 3. Make the no-PR guard fall back to GitHub before flagging needs-human

**File:** `saga/src/saga/orchestrator/steps/implementation/__init__.py` (lines 287-303)

When the state read returns no PR but review is enabled, ask GitHub directly: for each in-scope repo, is there an open PR on `ts.branch_name`? If yes, record it via `append_pr` and proceed to review. Only flag needs-human if GitHub also confirms there is no PR (or if we have no way to ask GitHub).

Replace the `if review_enabled and not ts.prs: ...` block with:

```python
review_enabled = ctx.deps.pr_monitor is not None

if review_enabled and not ts.prs:
    # State says no PR — but open_pr may have written prs and the read missed it
    # (state-store TOCTOU pre-FORGE-18 race, or eventually-consistent filtered
    # re-read). Before flagging, ask GitHub directly for any open PR on the
    # ticket's branch in each in-scope repo, and adopt it if found.
    recovered = await _recover_prs_from_github(ctx, ts)
    if recovered:
        ts = await task_state_repo.get(task.id) or TaskState()
        nums = ", ".join(f"{p.repo}#{p.number}" for p in ts.prs)
        logger.warning(
            f"implementation recovered PR(s) from GitHub after empty state read "
            f"task={task.id} prs={nums}"
        )
    else:
        logger.warning(f"implementation produced no PR task={task.id}; flagging needs-human")
        await ctx.deps.session_mgr.close(task.id)
        await task_state_repo.update_state(task.id, prs=[])
        await ctx.deps.mark_locally_failed(
            task.id,
            "Implementation finished without opening a PR — it likely hit a blocker "
            "(e.g. failing checks or verification). Re-run once resolved, or take it over "
            "manually.",
        )
        return
```

Add the helper alongside the other module helpers in the same file:

```python
async def _recover_prs_from_github(ctx: StepCtx, ts: TaskState) -> bool:
    """Last-resort PR lookup: ask GitHub for any open PR on ts.branch_name across
    in-scope repos. If found, persist them via append_pr and return True.

    Used to disambiguate "agent skipped open_pr" from "open_pr ran but state
    didn't reflect it" before the post_step no-PR guard fires.
    """
    github = ctx.deps.github
    if github is None or not ts.branch_name:
        return False
    found_any = False
    for repo_slug in _allowed_pr_repos(ctx.cfg, ts):
        try:
            pr = await github.get_pull_by_branch(repo_slug, ts.branch_name)
        except Exception:
            logger.exception(
                f"recover-from-github: get_pull_by_branch failed task={ctx.task.id} "
                f"repo={repo_slug} branch={ts.branch_name}"
            )
            continue
        if pr is None:
            continue
        await task_state_repo.append_pr(
            ctx.task.id,
            PRState(repo=repo_slug, number=pr.number, last_head_sha=pr.head_sha),
        )
        logger.info(
            f"recover-from-github: adopted PR task={ctx.task.id} "
            f"repo={repo_slug} pr={pr.number}"
        )
        found_any = True
    return found_any
```

Add `from saga.schemas.state import PRState, StepArtifact, TaskState` (PRState is a new import here).

This keeps the original intent of the guard (catch genuine "agent hit a blocker and gave up") because:
- If the agent really never called `open_pr` and never pushed a branch, `get_pull_by_branch` returns `None` for every repo — the guard fires as before.
- If `open_pr` ran end-to-end (which proves a PR exists on GitHub), we recover deterministically — no false-positive can survive.

### 4. Update the test fixture to add `append_pr`

**File:** `saga/tests/conftest.py`

Add to the `task_states` fixture, alongside the other fakes:

```python
async def fake_append_pr(task_id: str, pr) -> None:
    current = store.get(task_id, TaskState())
    updated_data = current.model_dump()
    updated_data["prs"] = [p for p in current.prs if p.repo != pr.repo] + [pr.model_dump()]
    store[task_id] = TaskState.model_validate(updated_data)

monkeypatch.setattr(_repo, "append_pr", fake_append_pr)
```

### 5. Tests

**File:** `saga/tests/test_implementation_step.py`

Add two new tests around the existing `test_post_step_no_pr_flags_needs_human_and_does_not_advance`:

1. `test_post_step_recovers_pr_from_github_when_state_is_empty` — `ts.prs == []` and `ts.branch_name` set; mock `ctx.deps.github.get_pull_by_branch` to return a PullRequest; assert: `mark_locally_failed` is **not** called, `tracker.write_state` is awaited with the In-Review transition, and `task_states["issue-1"].prs` ends up non-empty with the recovered PR. This is the FORGE-18 reproduction.

2. `test_post_step_no_pr_flags_when_github_also_has_no_pr` — `ts.prs == []`, `ts.branch_name` set, mock `get_pull_by_branch` to return `None` for every repo; assert the guard still fires (mirror of the existing test but with the GitHub fallback exercised). This nails down success criterion 2: the guard's original intent is preserved.

Helper change to support these: add a `_ctx_with_pr_monitor_and_github` (or a kwarg) so the existing `_ctx_with_pr_monitor` can plumb a mock `github` into `RunnerDeps` — today it hard-codes `github=None`.

**File:** `saga/tests/test_mcp_open_pr.py`

Update existing tests that introspect `task_states["t-1"].prs` to keep passing — they should, because `append_pr` produces the same end state. Add one new test:

3. `test_open_pr_concurrent_writes_dont_lose_a_pr` — call the underlying `open_pr` tool twice (different repos) in `asyncio.gather`. Assert both PRs land in `ts.prs`. This is the regression test for issue 1 (the lost-update race). With the old `update_state(prs=[*ts.prs, new_pr])` pattern this can lose a write; with `append_pr` it cannot.

**File:** `saga/tests/test_task_state_store.py` (if it exists; else extend `test_implementation_step.py`)

Add a direct unit test for `append_pr`:

4. `test_append_pr_appends_and_replaces_by_repo` — seed `ts.prs = [PRState(repo="a", number=1)]`, call `append_pr` for `("a", 2)` (should replace) and `("b", 3)` (should append). Assert the final list.

## Order of changes

1. Add `append_pr` to `task_state_store.py` and to the `conftest` fake — pure addition, no callers yet.
2. Add the standalone unit test for `append_pr`.
3. Switch `open_pr` to call `append_pr`. Run existing `test_mcp_open_pr.py` — should stay green.
4. Add the concurrent-writes regression test for `open_pr`.
5. Add `_recover_prs_from_github` helper + wire into the implementation post_step guard.
6. Add the two new implementation-step tests.
7. `just lint && just test`.

## Edge cases & risks

- **`ts.branch_name` is None.** Recovery is skipped (the helper returns False) and the guard fires as before — correct, since without a canonical head branch we can't disambiguate.
- **`github_client is None`** (development / test config without GitHub). Recovery is skipped, guard fires as before.
- **Two open PRs against different repos on the same branch.** `_recover_prs_from_github` adopts both; matches the multi-repo model already documented in `TaskState.prs`.
- **`get_pull_by_branch` is slow/flaky.** Each call is wrapped in `try/except`; a single repo's failure logs and continues to the next. Total latency is bounded by `len(in_scope_repos)` GitHub calls — small in practice.
- **PR was opened against a non-in-scope repo (out-of-scope guard scenario).** Already handled by the earlier `out_of_scope` block at lines 269-285, which runs **before** the no-PR guard. `_recover_prs_from_github` iterates `_allowed_pr_repos`, so it cannot resurrect an out-of-scope PR.
- **Race interaction with `pr_monitor`.** `pr_monitor.py:91` also writes `prs` via the old `update_state(prs=found)` pattern. Out of scope for this ticket (the bug fires before pr_review runs), but worth noting in a follow-up: `pr_monitor` should also use `append_pr` for the same TOCTOU reason.
- **Idempotency.** `append_pr` replaces-by-repo, so a duplicate adoption (e.g. `open_pr` wrote prs and then `_recover_prs_from_github` also adopts the same PR on a later tick) is a no-op, not a duplication.

## Verification

### Run/check commands found

From `saga/justfile`:

- **Tests:** `just test` (or `uv run pytest`)
- **Lint + types:** `just lint` (ruff check + ruff format --check + ty check)
- **Single test:** `uv run pytest tests/test_implementation_step.py::test_post_step_no_pr_flags_needs_human_and_does_not_advance`

The relevant CLAUDE.md and `.claude/skills/code-checks/SKILL.md` confirm "a change is not done until `just lint && just test` pass." Tests mock Linear/GitHub at the boundary — no credentials needed.

### Before-state (observed in this worktree)

I cannot reproduce the production false-positive locally because it requires a live Linear backend (the suspected eventually-consistent search filter) and a live Claude agent session. What I confirmed instead, as the closest available baseline:

- `uv run pytest tests/test_implementation_step.py tests/test_mcp_open_pr.py -v` — **28 passed**, including:
  - `test_post_step_no_pr_flags_needs_human_and_does_not_advance` (the guard fires when state is empty — current behavior we want to preserve when GitHub also has no PR).
  - `test_post_step_in_scope_pr_enters_review` (the guard does NOT fire when state has an in-scope PR — current happy path).
- Code-level: `open_pr` writes via `update_state(prs=[*ts.prs, new_pr])` (lock-free read), `_merge` overwrites the list field, no GitHub-side recovery exists in the guard. This is the surface the bug rides.

### After-state (what to observe once implemented)

- `uv run pytest tests/test_implementation_step.py tests/test_mcp_open_pr.py -v` — all existing tests still pass, plus the four new tests pass.
- The new `test_post_step_recovers_pr_from_github_when_state_is_empty` is the FORGE-18 reproduction proxy: with state empty and GitHub returning a PR, the post_step records the PR and advances to In-Review. This is success criterion 1.
- The new `test_post_step_no_pr_flags_when_github_also_has_no_pr` exercises the GitHub-fallback path returning empty; the existing `test_post_step_no_pr_flags_needs_human_and_does_not_advance` continues to pass. This is success criterion 2.
- `just lint` is clean (ruff + format + ty).

No artifact directory output is meaningful here (this is a backend-only behavioral fix with no UI to screenshot); the CI output from `just lint && just test` is the verification artifact.

## Out of scope (per ticket)

- Linear GraphQL client / comment persistence changes (no fix to the suspected eventually-consistent filter; we route around it via GitHub).
- PR creation logic in `GitHubClient` (untouched).
- Multi-repo specific handling beyond reusing the existing `_allowed_pr_repos` set.
- Fixing the same TOCTOU pattern in `pr_monitor.py:91` (flagged for a follow-up).
