# FORGE-5 — Load/clone only the relevant repos from triage

## Goal

Make `WorkspaceManager.create_workspace()` accept an optional repo filter, and have the two call sites (`StepRunner.run`, `PrMonitor._dispatch_turn`) feed it `TaskState.triage.repos` so that — after triage has run — only the triaged repos are cloned/worktreed for the rest of the ticket's lifecycle.

Triage itself still gets all repos (it has no triage result yet, by definition).

## Run / check commands

This branch ships under `saga/`. The orchestrator can't actually be run end-to-end without `LINEAR_OAUTH_TOKEN` + GitHub App creds, but the interface this ticket touches is fully exercised by `pytest`. The harness:

- **Lint + types:** `cd saga && uv run ruff check && uv run ruff format --check && uv run ty check` (== `just lint`).
- **Tests:** `cd saga && uv run pytest -q` (== `just test`). Targeted: `uv run pytest tests/test_workspace.py tests/test_flow_runner.py tests/test_orchestrator_pr_review.py tests/test_orchestrator_session_lifecycle.py tests/test_cancel_cleanup.py tests/test_archive_run.py tests/test_linear_app_orchestrator.py`.

Baseline observed in this worktree: `uv run pytest -q` → **696 passed**. `uv run ruff check` → **All checks passed**. The "before" CLI output is captured at `.saga/artifacts/before-cli-output.txt`.

## Behaviour today (verified by reading the code)

- `WorkspaceManager.create_workspace(task_id)` at `src/saga/services/git/workspace.py:177-198` iterates `self._repos.items()` unconditionally and clones/worktrees **every configured repo**.
- `StepRunner.run` (`src/saga/orchestrator/steps/runner.py:36-40`) calls `create_workspace(task.id)` **before** it reads `TaskState`, so it has no way to know which repos triage picked.
- `PrMonitor._dispatch_turn` (`src/saga/orchestrator/steps/review/pr_monitor.py:362-365`) likewise calls `create_workspace(task.id)` first, then reads `TaskState`.
- `ImplementationStep` already narrows its **in-process** repo iteration to the triaged set via `_target_repo_keys(cfg, ts)` at `src/saga/orchestrator/steps/implementation/__init__.py:85-88` — but that's downstream of an over-cloned workspace.
- `update_workspace`, `cleanup_workspace`, `checkout_branch`, `reset_for_replan`, `cleanup_orphan_branches` all iterate `self._repos` and **already** skip per-repo when the worktree dir is missing (`if not target.exists(): continue`). They don't need new filtering — they degrade cleanly to whatever subset is on disk.

## What needs to change

### 1. `WorkspaceManager.create_workspace()` — accept an optional `repos` filter

In `src/saga/services/git/workspace.py`:

- Change the signature to:
  ```python
  async def create_workspace(self, task_id: str, repos: list[str] | None = None) -> Path:
  ```
- Build a `selected: dict[str, RepoCfg]` at the top of the body:
  - `repos is None` → `selected = self._repos` (current full-clone behaviour, the fallback).
  - `repos is not None` → `selected = {k: self._repos[k] for k in repos if k in self._repos}`.
    - Silently drop unknown keys — `ts.triage.repos` is already validated by the `record_triage` MCP tool, but defending against config drift / a renamed repo is cheap.
- Iterate `selected.items()` instead of `self._repos.items()` (only that single loop changes).
- Update the existing `INFO` log to reflect the chosen subset: replace `repos={list(self._repos)}` with `repos={list(selected)}` so logs disambiguate "all" vs "filtered".

No other method needs to take the filter. `update_workspace` / `cleanup_workspace` / `checkout_branch` / `reset_for_replan` already iterate `self._repos` and `continue` on missing worktrees — that's the right behaviour for a partially-populated workspace (success criterion 4: "no errors or orphaned directories"). Explicitly threading the filter through them would add API surface without behaviour change.

### 2. `StepRunner.run` — read `TaskState` first, then pass the filter

In `src/saga/orchestrator/steps/runner.py`:

- Move the `ts = await task_state_repo.get(task.id) or TaskState()` line **above** the `if step.eager_workspace:` block (it's currently at line 40, just after the workspace calls).
- Compute `triaged = ts.triage.repos if (ts.triage and ts.triage.repos) else None` — mirrors the implementation step's existing `_target_repo_keys` shape, collapsing the empty-list case to `None` so the fallback path is taken.
- Pass `repos=triaged` to `create_workspace`: `await self._deps.workspace.create_workspace(task.id, repos=triaged)`.
- `update_workspace(task.id)` does not change.

This preserves the current behaviour for the triage step (where `ts.triage is None` → `triaged = None` → clone all) and narrows every subsequent step (`product_definition`, `technical_plan`, `implementation`) to the triaged set on first clone. On orchestrator restart or after `cleanup_workspace`, only the triaged repos are re-cloned — which is the actual disk/time saving the ticket is after.

### 3. `PrMonitor._dispatch_turn` — same change

In `src/saga/orchestrator/steps/review/pr_monitor.py` `_dispatch_turn` (around line 362-365):

- Move the `ts = await task_state_repo.get(task.id) or TaskState()` line above `create_workspace`.
- Compute the same `triaged = ts.triage.repos if (ts.triage and ts.triage.repos) else None`.
- Pass `repos=triaged` to `create_workspace`.

By the time PR review runs, triage has always completed, so in normal operation `triaged` will be non-None — but the same `None` fallback applies for defensiveness (e.g. legacy task states pre-dating triage).

### 4. Update test fakes that implement `create_workspace`

Four test files define `_FakeWorkspace.create_workspace` or `FakeWorkspace.create_workspace`. The new keyword argument has a default, but the fakes must still accept it so the production call signature matches when callers pass `repos=...`:

- `tests/test_cancel_cleanup.py:59`
- `tests/test_archive_run.py:60`
- `tests/test_orchestrator_pr_review.py:157`
- `tests/test_orchestrator_session_lifecycle.py:71`

Change each to:

```python
async def create_workspace(self, task_id: str, repos: list[str] | None = None) -> Path:
    return self._workdir
```

In `tests/test_flow_runner.py` the workspace is an `AsyncMock`, so it already accepts any keyword arg and no change is needed.

### 5. New tests for the filter behaviour (success criteria 1 + 2)

Add to `tests/test_workspace.py` (next to the other `create_workspace` tests):

- **`test_create_workspace_clones_all_repos_by_default`** — build a manager with two repos `app` + `api`, bare-init both, call `create_workspace("FEAT-1")` with no filter, assert that both `ws/app/` and `ws/api/` exist as worktrees (covers success criterion 2: fallback to all when no triage).
- **`test_create_workspace_with_repos_filter_clones_only_selected`** — same two-repo setup, call `create_workspace("FEAT-2", repos=["app"])`, assert `ws/app/` exists and `ws/api/` does **not**. Also assert that the bare clone for `api` was **not** created (the bare-dir check is the cheapest "did we touch this repo" assertion: `bare_dir(tmp_path, "api").exists()` should be False).
- **`test_create_workspace_filter_ignores_unknown_keys`** — call `create_workspace("FEAT-3", repos=["app", "ghost"])`; assert `ws/app/` exists, no error raised, `ws/ghost/` does not exist.
- **`test_create_workspace_filter_none_falls_back_to_all`** — explicit `repos=None` call clones every configured repo (defensive; the same as default).

Helper extension: `_make_manager` currently takes a single `repo_name`. Add a variant (or inline) that takes a list of repo names and initialises a bare for each, so the multi-repo tests can share setup with `_init_bare_with_commit`.

### 6. Optional cleanup-respects-filter sanity test (success criterion 4)

Add `test_cleanup_workspace_handles_partial_clone(tmp_path)` to `tests/test_workspace.py`: create a workspace with `repos=["app"]` (against a two-repo manager), then call `cleanup_workspace("FEAT-X")` and assert it returns without raising and the workspace dir is gone. This locks in the "no errors on partial workspaces" guarantee. (Existing code already supports this; the test pins it.)

### 7. Doc note

In `saga/docs/workspace.md`, the line "Saga currently does not distinguish repo-bound and repo-less tasks at the workspace layer. Linear repo labels are not used for workspace selection; each task sees all configured repos." is now stale. Replace with one sentence noting that after triage, only `TaskState.triage.repos` are cloned, while pre-triage / no-triage tasks see all configured repos. Keep it short — the rest of the doc is layout-and-cleanup focused and doesn't change.

## Order of changes

The dependencies push a natural order:

1. **`WorkspaceManager.create_workspace`** — the API change is the foundation; everything else calls it.
2. **Test fakes** — update them in lockstep so the test suite still compiles. (Doing this before the call-site changes prevents an intermediate broken state if `pytest` is run.)
3. **`StepRunner.run`** — pass the filter.
4. **`PrMonitor._dispatch_turn`** — pass the filter.
5. **New tests in `test_workspace.py`** — pin the new contract.
6. **`docs/workspace.md`** — small wording update.

## Edge cases / risks

- **Empty `ts.triage.repos`** — triage's `post_step` already pauses for needs-human on an empty repo list (see `src/saga/orchestrator/steps/triage/__init__.py:162-176`), so downstream steps never reach the runner with an empty list. The `if (ts.triage and ts.triage.repos)` guard collapses this case to `None` (= all repos) anyway, matching `_target_repo_keys`.
- **Stale triage referencing a removed config repo** — silently dropped by the `if k in self._repos` filter in `create_workspace`. The orchestrator will continue with the remaining repos; the implementation step's `_target_repo_keys` already does the same intersection and so the runner workspace and the impl iteration stay consistent.
- **First post-triage tick after the worktree was lost** — the orchestrator restarts or the workspace was cleaned. `create_workspace` will clone only the triaged repos. Correct, and this is the path where the disk/time saving lands.
- **First post-triage tick when the workspace was built during triage with all repos** — `create_workspace` is idempotent on existing worktrees, so the non-triaged worktrees stay on disk (no error). This is an acceptable "transitional" state — the next `cleanup_workspace` clears the whole task dir. We are intentionally not actively pruning non-triaged worktrees in this ticket (out of scope per the ticket's "Out of scope" line on `_target_repo_keys`); the ticket asks to avoid cloning, not to retroactively delete.
- **pr_review's lazy workspace** — `PrMonitor._dispatch_turn` runs only on actionable comments / CI / conflict; passing the filter means the lazy clone after a workspace reset only fetches the triaged repos. No regression for the no-clone case.
- **`StepCtx.ts` vs the post-create state** — the runner reads `ts` once and uses it both for the filter and the StepCtx. Triage's own dispatch reads `ts` (no triage yet → all repos), then writes triage during its `work`, then writes a fresh ts on next tick. No re-ordering hazard.
- **Bare clone reuse** — `ensure_bare` is gated on `bare.exists()`. A repo that's in cfg but never triaged for any task in flight will simply never have its bare created. That's the intended saving and is harmless on the next ticket that does triage it in.

## Verification

1. **Lint + types pass:** `uv run ruff check && uv run ruff format --check && uv run ty check`.
2. **Full pytest passes:** `uv run pytest -q` should remain ≥ the baseline 696 + the new tests added.
3. **Workspace tests specifically:** `uv run pytest tests/test_workspace.py -q` — must include the new filter tests and pass.
4. **Behavioural observation (before vs after):**
   - **Before** (baseline captured in `.saga/artifacts/before-cli-output.txt`): inspecting `create_workspace` shows it unconditionally iterates `self._repos`. The existing tests in `test_workspace.py` all use single-repo configs, so the over-clone is invisible at the test layer today.
   - **After:** new `test_create_workspace_with_repos_filter_clones_only_selected` proves a two-repo manager called with `repos=["app"]` produces a workspace containing only `app/` (and no bare for `api`). New `test_create_workspace_clones_all_repos_by_default` proves the fallback path is unchanged. Together these are the verifiable evidence for success criteria 1 and 2.
5. **Artifact capture:** after implementation, write `.saga/artifacts/after-cli-output.txt` with the final pytest + ruff output and a one-liner `find` showing the per-task worktree contains only the filtered repos (e.g. via the new test's tmp_path probe).

## Out of scope (per the ticket)

- Changing what triage classifies.
- Modifying `TriageResult` schema or step order.
- Touching `_target_repo_keys` in the implementation step (already correct; we are bringing the same logic upstream into the workspace layer).
- Actively pruning non-triaged worktrees that were created before this change. Existing workspaces are left alone; new workspaces (post-cleanup, post-restart, or on a freshly triaged ticket) get the filter benefit.
