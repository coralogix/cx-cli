# FORGE-254 — Friendly `needs_human` comment when workspace clone fails

## Summary

The bug is **not** that saga crashes on a missing clone — it already degrades to a `needs-human` comment (via `StepRunner.run`'s generic `except Exception`, `src/saga/orchestrator/steps/runner.py:59-74`, calling `format_exception` at `src/saga/util.py:17-27`). The problem is that this fallback dumps the full multi-frame Python traceback of `run_git → ensure_bare → create_workspace` as the *entire* Linear comment body. To the human reading FORGE-252, the traceback made it look like saga couldn't find the repo in the config, when actually GitHub was returning "Repository not found" for a `git clone --bare` — most likely a GitHub App installation-access issue on `coralogix/olly-knowledge-base`.

The fix is presentational: when workspace setup fails for a specific configured repo, we (a) wrap the raised `GitError` with the failing repo's config key, and (b) let the step runner render a short, human-readable comment for that specific exception type, keeping the current traceback fallback for every other exception (preserves the FORGE-20 / FORGE-30 "surface exception details" behavior for anything unclassified).

Underlying access/config for `olly-knowledge-base` itself is explicitly out of scope (per ticket).

## Files to change

### 1. `src/saga/services/git/workspace.py` — new exception + wrap `create_workspace`

Add a new `WorkspaceError` subclass so the runner can pattern-match:

```python
class WorkspaceSetupError(WorkspaceError):
    """Raised when workspace prep for a specific configured repo fails.

    Wraps the underlying GitError so the failing repo's config key is preserved for
    the needs-human comment; str(exc) reproduces the git command + stderr already
    captured in GitError.__str__, so no information is lost."""

    def __init__(self, repo_name: str, cause: GitError) -> None:
        self.repo_name = repo_name
        self.cause = cause
        super().__init__(f"repo `{repo_name}` could not be prepared: {cause}")
```

In `WorkspaceManager.create_workspace` (`workspace.py:191-216`), wrap the per-repo body of the loop so any `GitError` from `ensure_bare`, `resolve_base_branch`, or the `worktree add` call becomes a `WorkspaceSetupError` carrying the repo key:

```python
for name, cfg in selected.items():
    target = ws / name
    if target.exists():
        logger.info(...)
        continue
    logger.info(...)
    try:
        bare = await ensure_bare(self.root, name, cfg.clone_url())
        base = await resolve_base_branch(bare, cfg.base_branch)
        logger.info(...)
        await run_git(bare, ["worktree", "add", "--detach", str(target), origin_ref(base)])
    except GitError as exc:
        raise WorkspaceSetupError(repo_name=name, cause=exc) from exc
    logger.info(...)
```

Notes:
- `WorkspaceSetupError` subclasses `WorkspaceError`, so existing broad catches (`WorkspaceError` in `implementation/__init__.py:124`, `pr_monitor.py`) still trigger and behavior is unchanged there.
- We wrap only `GitError` (not any `WorkspaceError`) so future `WorkspaceError` types raised in-loop don't get accidentally reclassified.
- We do **not** touch `run_git`, `ensure_bare`, or `resolve_base_branch` — keeping their signatures/exceptions stable avoids ripples through `test_workspace.py`.
- `update_workspace` (called next in `runner.py`) already swallows `WorkspaceError` inline, so it can't leak the wrong error class.

### 2. `src/saga/orchestrator/steps/runner.py` — render friendly message for `WorkspaceSetupError`

At the top of the file, add the import next to the existing workspace import (the runner does not import from workspace.py today; add a fresh line):

```python
from saga.services.git.workspace import WorkspaceSetupError
```

Change the exception handling block (`runner.py:59-74`) to catch `WorkspaceSetupError` specifically **before** the generic `except Exception`. Order matters: the transient-error check must remain inside the generic block (it applies only to `LinearHttpError`).

```python
except WorkspaceSetupError as exc:
    logger.exception(f"workspace setup failed task={task.id} step={step.name}")
    await self._deps.mark_locally_failed(
        task.id,
        (
            f"`{step.name}` could not prepare the workspace for repo "
            f"`{exc.repo_name}`.\n\n```\n{exc.cause}\n```"
        ),
    )
except Exception as exc:
    if is_transient_error(exc):
        logger.warning(...)
        return
    logger.exception(f"step execution error task={task.id} step={step.name}")
    detail = format_exception(exc)
    await self._deps.mark_locally_failed(
        task.id,
        f"`{step.name}` raised an error.\n\n```\n{detail}\n```",
    )
```

What the human now sees on FORGE-252-shaped failures:

```
`triage` could not prepare the workspace for repo `knowledge-base`.

git command `git clone --bare git@github.com:coralogix/olly-knowledge-base.git /data/workspaces/knowledge-base/.bare` failed: exit 128: Cloning into bare repository '/data/workspaces/knowledge-base/.bare'...
remote: Repository not found.
fatal: repository 'https://github.com/coralogix/olly-knowledge-base.git/' not found
```

Names the repo, surfaces the git command + stderr, no Python frames.

### 3. `tests/test_flow_runner.py` — regression test for the friendly path

Sibling of `test_run_create_workspace_raises_marks_locally_failed` (`tests/test_flow_runner.py:373-389`), which stays untouched so we still cover the "unknown exception → traceback dump" fallback. Add a new test that stubs `workspace.create_workspace` to raise a real `WorkspaceSetupError`:

```python
async def test_run_create_workspace_git_error_marks_locally_failed_with_friendly_message(
    task_states: dict,
) -> None:
    """A GitError during workspace setup is surfaced with the repo name and the git
    stderr — not a raw Python traceback."""
    from saga.services.git.workspace import GitError, WorkspaceSetupError

    cfg = _cfg()
    task_states["issue-1"] = TaskState(stage=Stage.WORKING)
    mark_failed = AsyncMock()
    cause = GitError(
        "git clone --bare git@github.com:coralogix/olly-knowledge-base.git "
        "/data/workspaces/knowledge-base/.bare",
        "exit 128: remote: Repository not found.\nfatal: repository ... not found",
    )
    workspace = AsyncMock()
    workspace.create_workspace = AsyncMock(
        side_effect=WorkspaceSetupError(repo_name="knowledge-base", cause=cause)
    )
    workspace.update_workspace = AsyncMock()
    deps = _deps(cfg, mark_failed=mark_failed)
    # Swap the workspace stub after _deps() so we keep the rest of its wiring.
    deps = deps.model_copy(update={"workspace": workspace})

    step = _RunnerStep(name="triage", work=_work(_Spy(), WorkStatus.DONE), pre_step=None)
    registry = _registry(step)
    runner = StepRunner(deps, registry)

    await runner.run(step, _task())

    mark_failed.assert_awaited_once()
    task_id, reason = mark_failed.call_args[0]
    assert task_id == "issue-1"
    assert "`triage`" in reason
    assert "knowledge-base" in reason
    assert "Repository not found" in reason
    # Friendly path: no traceback, no Python module paths.
    assert "Traceback" not in reason
    assert "runner.py" not in reason
    assert "WorkspaceSetupError" not in reason
```

Notes:
- We build the `WorkspaceSetupError` in the test rather than have the fixture raise a `GitError` that gets wrapped, because `_deps`'s `workspace.create_workspace` is a plain `AsyncMock`, not the real `WorkspaceManager.create_workspace`. Testing the friendly-comment path this way keeps this test scoped to the runner.
- The wrapping itself is covered by the `test_workspace.py` addition below.
- `_deps` is currently a helper (`tests/test_flow_runner.py:90-117`) that only supports `create_raises=True → RuntimeError("boom")`. Rather than growing its signature, the new test builds its own workspace mock and swaps it in via `model_copy`. (Alternative: extend `_deps` with a `create_side_effect: Exception | None` parameter. Slightly cleaner but wider surface — either is fine.)

### 4. `tests/test_workspace.py` — regression test for the wrapping

Add a small test asserting `create_workspace` re-raises as `WorkspaceSetupError` (with `repo_name` set to the config key) when the underlying clone fails. Use a `RepoCfg` whose `clone_url()` points to a non-existent local file path so `git clone` exits non-zero deterministically without any network / GitHub App dependency:

```python
async def test_create_workspace_wraps_clone_failure_with_repo_name(tmp_path: Path) -> None:
    """A GitError from ensure_bare is re-raised as WorkspaceSetupError with the repo key."""
    from saga.config import RepoCfg
    from saga.services.git.workspace import WorkspaceSetupError

    # Point the clone at a nonexistent local path so `git clone` fails fast without
    # touching GitHub / the credential helper.
    repos = {
        "knowledge-base": RepoCfg(github="does-not-exist/nope", base_branch="main"),
    }
    mgr = WorkspaceManager(root=tmp_path, repos=repos)

    with pytest.raises(WorkspaceSetupError) as exc_info:
        await mgr.create_workspace("FEAT-XX")

    assert exc_info.value.repo_name == "knowledge-base"
    # The friendly repr surfaces the git output (the stderr from the failed clone).
    assert "knowledge-base" in str(exc_info.value)
```

Add `import pytest` at the top of the file if not already imported (currently it isn't — the existing tests use assertion-only style). If we want to avoid introducing `pytest.raises`, we can switch to:

```python
raised: WorkspaceSetupError | None = None
try:
    await mgr.create_workspace("FEAT-XX")
except WorkspaceSetupError as exc:
    raised = exc
assert raised is not None
assert raised.repo_name == "knowledge-base"
assert "knowledge-base" in str(raised)
```

Prefer the `pytest.raises` form — it's idiomatic and `pytest` is already a dev dep.

**Caveat**: the real `run_git` in this repo injects a `saga git-credential` helper via `GIT_CONFIG_*` env vars (`workspace.py:59-66`). When the test's `git clone` runs, git will invoke that helper. If saga isn't installed as a CLI on `PATH` during test runs, git may print a helper-not-found warning but still fail as expected (exit 128 because the URL points at a nonexistent GitHub repo / SSH host). If this proves flaky, fall back to the try/except form and drop the underlying-output assertion — the essential contract is `raised.repo_name == "knowledge-base"`.

## Order of changes

1. Add `WorkspaceSetupError` and the loop-body try/except in `workspace.py` (source-of-truth change).
2. Add the specific-exception branch in `runner.py`.
3. Add the runner-level test in `tests/test_flow_runner.py`.
4. Add the workspace-level wrapping test in `tests/test_workspace.py`.
5. Run `just lint-fix && just lint && just test` from repo root and confirm all pass.

Steps 1 and 2 are the substantive change; 3 and 4 are the regression tests the ticket asks for.

## Edge cases / risks

- **Preserved fallback:** the generic `except Exception` (and its `is_transient_error` short-circuit + `format_exception` traceback dump) stays intact. `test_run_create_workspace_raises_marks_locally_failed`, `test_run_work_raises_marks_locally_failed`, `test_run_transient_backend_error_retries_not_escalates`, and `test_run_non_transient_http_error_escalates` (`tests/test_flow_runner.py:373-452`) must continue to pass unmodified. Explicitly a success criterion.
- **`WorkspaceSetupError` isa `WorkspaceError`:** every `except WorkspaceError` in the codebase (`implementation/__init__.py:124`, `pr_monitor.py` refs, `workspace.py`'s own internal cleanup catches) still triggers as before. No behavior change on those paths.
- **`update_workspace` failures**: `update_workspace` (called on the line after `create_workspace` in the runner) catches its own `WorkspaceError` and only logs — it can't leak an unwrapped `GitError` into the runner. If that ever changes, the friendly-comment path would still fire because a WorkspaceSetupError wouldn't be raised — the fallback traceback would be shown. Acceptable; explicitly out of scope.
- **Ordering of `except` blocks:** `WorkspaceSetupError` must come first because it's an `Exception` subclass — swapping order would trap it in the generic block. Guard with the new regression test.
- **Transient-error classification:** `is_transient_error` only returns True for `LinearHttpError`, so a `WorkspaceSetupError` cannot be misrouted to the retry path.
- **Message length:** git stderr is short (single-digit lines); `format_exception`'s 1500-char truncation is not needed for the friendly path. If some git failure ever produced huge stderr, `mark_locally_failed → needs_human → tracker.add_comment` may hit Linear's comment length limit — but the current traceback path has the same exposure, so this is not a regression.

## Verification

**Run/check commands** (from `justfile`; also documented in `.claude/skills/code-checks/SKILL.md`):

```bash
just lint-fix
just lint
just test
```

All three must pass. For focused iteration:

```bash
just test tests/test_flow_runner.py::test_run_create_workspace_git_error_marks_locally_failed_with_friendly_message
just test tests/test_workspace.py::test_create_workspace_wraps_clone_failure_with_repo_name
just test tests/test_flow_runner.py       # exception-wrapper coverage
just test tests/test_workspace.py         # create_workspace coverage
```

Always finish with the full `just test` before declaring done — a targeted run misses regressions in adjacent tests (e.g. `implementation/__init__.py`'s `except (WorkspaceError, OSError)` behavior).

**Cannot run the app end-to-end in this worktree** — `saga run` needs a live Linear board (`LINEAR_OAUTH_TOKEN`) and GitHub App credentials to reach the workspace-creation code path. This is a normal-for-this-repo constraint (per `code-checks/SKILL.md`: tests use fakes; no Linear needed for `just test`). The tests **are** the verification: they exercise the exact code path that produced FORGE-252's traceback comment and assert the new format.

**Before / after evidence:**
- **Before** (from FORGE-252, quoted in the ticket description, verbatim): the `needs_human` comment is `` `triage` raised an error. `` followed by an 8-frame Python traceback ending in `saga.services.git.workspace.GitError: git command ...`.
- **After** (asserted by the new runner test): `` `triage` could not prepare the workspace for repo `knowledge-base`. `` followed by a fenced block with just the `git command '...' failed: exit 128: ...Repository not found...` line. No `Traceback (most recent call last)` string, no saga module paths, no line numbers.
