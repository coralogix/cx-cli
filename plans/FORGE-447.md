# Technical plan — FORGE-447: linting-cache support for saga

## Goal in one line

Redirect per-repo, cacheable lint state (mypy in `olly/apps/api`, Nx's local cache in `frontend`, eslint's `.eslintcache` in `frontend`) from the ephemeral task worktree to a persistent VM-local location, so cache hits survive worktree teardown and are reused across tickets — without allowing two concurrent tasks against the same repo to corrupt each other's cache.

## Chosen concurrency strategy

**Per-task working copy + shared "warm" seed, protected by a per-cache flock on the warm seed only.**

- Each task gets its own private cache directory outside the worktree. Only that task writes there during its lifetime, so no two tools ever contend over a live cache file.
- A per-repo, per-cache "warm" seed directory is the shared long-lived state. New tasks copy from it at workspace-create time; completed tasks copy back to it at cleanup time. Every seed/write-back holds an `fcntl.flock` on `<cache>/warm.lock` — shared for reads, exclusive for writes.
- Last-writer-wins on the warm seed is fine: both writers derived their cache from the same warm base, so worst case is one task's incremental updates are lost (perf regression on the *next* task's cold entries, not correctness).

Rejected alternatives:
- **Single shared cache dir with a symlink and no protection**: mypy/eslint aren't concurrency-safe writers; even though their caches are self-validating on read, two concurrent writes on the same cache file can produce corrupt entries that then produce cache misses forever (dead entries). Fails success criterion #4.
- **Per-base-branch shared cache with an flock around the lint invocation**: would require wrapping `mypy`/`nx`/`eslint` via `/opt/saga/shims`, which is invasive and cross-cuts every downstream repo's build. Also doesn't help when two tasks share the same base branch (the common case).
- **Hardlink copy of warm into task dir**: mypy rewrites cache files in place; a hardlink copy would mutate the warm cache mid-task. Unsafe.

## Persistent layout

Extends the existing `<workspace.root>/<repo_name>/` directory (currently home to `.bare/`) with a sibling `.cache/`:

```
<workspace.root>/
├── olly/
│   ├── .bare/
│   └── .cache/
│       └── apps_api_.mypy_cache/            # one entry per configured cache path
│           ├── warm/                        # long-lived seed
│           ├── warm.lock                    # flock file (empty)
│           └── tasks/
│               ├── forge-447/               # per-task working copy
│               └── forge-448/
├── frontend/
│   ├── .bare/
│   └── .cache/
│       ├── .nx_cache/
│       │   ├── warm/
│       │   ├── warm.lock
│       │   └── tasks/forge-447/
│       └── .eslintcache/
│           ├── warm/         # holds a single file: eslintcache
│           ├── warm.lock
│           └── tasks/forge-447/eslintcache
└── tasks/
    └── forge-447/
        ├── olly/
        │   └── apps/api/.mypy_cache        →  ../../../../olly/.cache/apps_api_.mypy_cache/tasks/forge-447/
        └── frontend/
            ├── .nx/cache                    →  ../../../frontend/.cache/.nx_cache/tasks/forge-447/
            └── .eslintcache                 →  ../../../frontend/.cache/.eslintcache/tasks/forge-447/eslintcache
```

The `<sanitized_key>` in `.cache/<sanitized_key>/` is the cache path with `/` → `_` (via the existing `sanitize()` helper), so two configured caches never collide.

## Config surface

Add an optional `caches` field to `RepoCfg` (`src/saga/config.py`). Each entry names a path relative to the repo worktree root, plus its kind:

```python
class CacheKind(str, Enum):
    dir = "dir"      # default; the path IS the cache directory
    file = "file"    # the path is a single cache file (e.g. `.eslintcache`)


class CacheEntry(StrictModel):
    path: str
    kind: CacheKind = CacheKind.dir


class RepoCfg(StrictModel):
    github: str
    base_branch: str = "main"
    caches: list[CacheEntry] = Field(default_factory=list)
```

Backward compat: `caches` defaults to `[]`, so every existing config gets today's behavior. All `StrictModel`s already reject unknown top-level fields, so operators can only opt in explicitly.

Wire it up in `examples/linear.yaml` as a documented, commented-out example:

```yaml
repos:
  olly:
    github: coralogix/olly
    base_branch: main
    caches:
      - path: apps/api/.mypy_cache
  frontend:
    github: coralogix/frontend
    base_branch: main
    caches:
      - path: .nx/cache
      - path: .eslintcache
        kind: file
```

The actual production config (deployed via `deploy/`) is out-of-scope for the saga source PR — a separate deploy-config change enables it against the real repos.

## Code changes (`src/saga/services/git/workspace.py`)

All changes are additive; existing signatures stay backward-compatible.

### New helpers (module-level)

```python
def cache_root(workspaces_root: Path, repo_name: str) -> Path:
    return workspaces_root / repo_name / ".cache"

def _cache_dir(workspaces_root: Path, repo_name: str, entry: CacheEntry) -> Path:
    # one subdir per configured cache; key sanitized to avoid path chars
    return cache_root(workspaces_root, repo_name) / sanitize(entry.path)

def _warm_path(cache_dir: Path) -> Path:
    return cache_dir / "warm"

def _task_path(cache_dir: Path, task_id: str) -> Path:
    return cache_dir / "tasks" / sanitize(task_id)

def _lock_path(cache_dir: Path) -> Path:
    return cache_dir / "warm.lock"
```

### `_with_flock(lock_file: Path, exclusive: bool)` async context manager

Uses `asyncio.to_thread` to open the lock file and call `fcntl.flock(fd, LOCK_EX or LOCK_SH)`, yields, then releases. `fcntl` is stdlib on Linux/macOS. The lock file itself is a zero-byte placeholder created lazily.

### `_seed_task_cache(root, repo_name, repo_cfg, task_id, worktree)`

Called from `create_workspace` after `git worktree add` succeeds. For each `CacheEntry` in `repo_cfg.caches`:

1. Compute `cache_dir`, `warm`, `task_dir`, `lock`.
2. `cache_dir.mkdir(parents=True, exist_ok=True)`; touch `lock`.
3. Under **shared** flock on `lock`:
   - Ensure `warm` exists (`warm.mkdir(exist_ok=True)`).
   - If `task_dir` already exists, skip seeding (idempotent — the task workspace is being reused).
   - Else copy the warm seed into `task_dir` in a thread:
     - `dir` kind: `shutil.copytree(warm, task_dir, symlinks=False, dirs_exist_ok=True)`.
     - `file` kind: `task_dir.mkdir(parents=True)`; if `warm/<basename>` exists, `shutil.copyfile` it to `task_dir/<basename>`; else `Path(task_dir/basename).touch()`.
4. Create the in-worktree symlink pointing at the seeded location:
   - Ensure parents of `<worktree>/<entry.path>` exist (`mkdir(parents=True, exist_ok=True)`).
   - If the target exists as a real dir/file (e.g. the repo checks a `.mypy_cache/` into VCS — it shouldn't, but be defensive), delete it first: `shutil.rmtree` for dir, `unlink` for file.
   - `dir` kind: `os.symlink(task_dir, <worktree>/<entry.path>)`.
   - `file` kind: `os.symlink(task_dir/<basename>, <worktree>/<entry.path>)`.
5. Any failure inside this helper is logged as a warning and swallowed — a broken cache setup must not fail workspace creation. Cold lint runs are worse than nothing, not a correctness bug.

### `_write_back_task_cache(root, repo_name, repo_cfg, task_id)`

Called from `cleanup_workspace` *before* removing the worktree. For each `CacheEntry`:

1. Compute paths.
2. If `task_dir` doesn't exist, skip.
3. Under **exclusive** flock on `lock`, in a thread:
   - `dir` kind: `shutil.copytree(task_dir, warm, dirs_exist_ok=True)`.
   - `file` kind: `shutil.copyfile(task_dir/basename, warm/basename)` if the source exists.
4. Then `shutil.rmtree(task_dir, ignore_errors=True)`.
5. Failures logged as warnings; swallowed.

Same helper is called from `cleanup_stale_workspaces` (per orphaned task id) before it invokes `cleanup_workspace`.

### Modified methods on `WorkspaceManager`

- `create_workspace`: after `run_git(bare, ["worktree", "add", ...])` succeeds for a given repo, call `await _seed_task_cache(self.root, name, cfg, task_id, target)`. Idempotent — safe to re-run on orchestrator restart.
- `cleanup_workspace`: before the existing `git worktree remove` / `_remove_tree(target)` steps for each repo, call `await _write_back_task_cache(self.root, name, cfg, task_id)`.
- `prune_workspace`: also call `_write_back_task_cache` for each pruned repo — the cache built up in triage's initial full-repo clone is worth preserving.
- `cleanup_stale_workspaces`: for each orphan entry, before invoking `cleanup_workspace`, iterate the *configured* repos and call `_write_back_task_cache` for each — the orphaned worktree may have live cache updates worth preserving.
- `reset_for_replan`: **no change**. The per-task cache directory is repo-scoped, not attempt-scoped — keeping it warm across replans is a strict win.

## Concurrency correctness — walkthrough

Two tasks A and B run `just lint` in `olly` simultaneously:

1. A starts, `create_workspace(A)` runs. Shared flock, copies warm → tasks/A/. Releases.
2. B starts, `create_workspace(B)` runs. Shared flock (compatible with A's already-released shared flock or a concurrent shared flock). Copies warm → tasks/B/. Releases.
3. A's `just lint` writes to `tasks/A/.mypy_cache/…`. B's writes to `tasks/B/.mypy_cache/…`. Fully isolated — no shared inode, no shared file.
4. A finishes first. `cleanup_workspace(A)` acquires **exclusive** flock, merges tasks/A/ into warm/, removes tasks/A/, releases.
5. B finishes. `cleanup_workspace(B)` acquires exclusive flock (waits if A's was still held), merges tasks/B/ into warm/, overwriting A's entries where they conflict. Removes tasks/B/, releases.
6. Task C starts later: seeds from the merged warm containing (B's ∪ A's non-B) entries. Slight loss of A's newest entries where they overlapped with B's; C revalidates or recomputes them.

There is one race: two tasks landing at cleanup exactly overlapping. The exclusive lock serializes them, so the merge is atomic per-task — no half-copied files land in warm. The lock is per-`CacheEntry`, so mypy write-back doesn't block nx write-back.

## Verification (implementation-step gate)

The saga repo itself is a Python tree; the ticket's real success criteria are exercised against `olly`/`frontend`. Split verification accordingly:

**Local (`just test && just lint` in saga):**
1. `just lint` — ruff + ty must be clean on the touched files (`src/saga/services/git/workspace.py`, `src/saga/config.py`, `tests/test_workspace.py`, docs).
2. `just test` — new unit tests in `tests/test_workspace.py` must pass:
   - `test_create_workspace_seeds_cache_from_warm`: pre-populate `<root>/<repo>/.cache/<key>/warm/foo.txt`, create workspace, assert the seeded file is visible under the symlink inside the worktree.
   - `test_create_workspace_creates_symlink_for_dir_cache`: assert `<worktree>/<cfg.path>` is a symlink pointing at `.cache/<key>/tasks/<task>/`.
   - `test_create_workspace_creates_symlink_for_file_cache`: same for `kind=file`.
   - `test_cleanup_workspace_merges_task_cache_into_warm`: write to the symlink inside the worktree, cleanup, assert file lands in `.cache/<key>/warm/`.
   - `test_cleanup_workspace_removes_task_cache_dir`: after cleanup, `tasks/<task>/` is gone.
   - `test_second_task_reuses_first_task_warm_cache`: run create-write-cleanup for task A, then create for task B, assert A's file is present under B's symlink.
   - `test_concurrent_writeback_serialized`: kick off two `cleanup_workspace` calls concurrently on overlapping tmp caches with distinct file trees, assert both files land in warm without corruption (both files present, both readable).
   - `test_create_workspace_without_caches_field_is_unchanged`: config with empty `caches` produces no `.cache/` subtree — regression guard.
   - `test_cleanup_stale_workspaces_writes_back_orphan_cache`: pre-populate a stale task's cache dir, run stale cleanup, assert warm has the file and the task dir is gone.

**Verifying success criteria (post-merge, outside this PR):**
Success criteria #1 and #2 require `olly` and `frontend` and cannot be reproduced in the saga worktree. The deploy-config change that enables the `caches:` entries in the real Linear config is the trigger; operators will observe by:

- Criterion #1: On the second saga task against `olly` after this ships, `just lint` in `olly/apps/api` runs mypy with `--verbose`; expect `LOG:  Metadata fresh for src/api/agent/…` cache-hit lines instead of `LOG:  Parsing …` cold-parse lines. Time-to-lint drops from cold to warm (baseline: measure once cold, once warm).
- Criterion #2: On the second saga task against `frontend`, Nx output shows `>  NX   Successfully ran target lint …` with a `Nx read the output from the cache instead of running the command` note. Set `NX_VERBOSE_LOGGING=true` if needed.
- Criterion #3: After `cleanup_workspace` runs (task moves to a terminal Linear status), `<workspace.root>/<repo>/.cache/<key>/warm/` still contains cache files. `<workspace.root>/<repo>/.cache/<key>/tasks/<sanitized_task_id>/` is gone.
- Criterion #4: Trigger two overlapping tasks against the same repo; confirm no `just lint` failure/false-positive across the pair and warm accumulates entries from both.

## Docs

- `docs/workspace.md`: append a new **"Persistent tool caches"** section describing the `<workspace.root>/<repo>/.cache/` layout, the warm+tasks split, the flock protocol, and the write-back-on-cleanup flow. Link to the new config field.
- `docs/config-schema.md`: extend the **Repos** section to document the `caches:` field, both `dir` and `file` kinds, with the same YAML example as `examples/linear.yaml`.
- `examples/linear.yaml`: add a commented-out `caches:` block on the `app:` sample repo, showing both dir and file cache entries with a comment pointing at `docs/workspace.md`.

No `CLAUDE.md` change needed — this is not a top-level behavior change, and CLAUDE.md deliberately defers detail to `docs/`.

## Order of implementation (dependencies first)

1. **Config schema** — add `CacheKind`, `CacheEntry`, `RepoCfg.caches` in `src/saga/config.py`. Update `test_config_phases.py`-adjacent tests to prove the new field validates and defaults empty. This must go first because everything else imports from `RepoCfg`.
2. **Helpers in `workspace.py`** — `cache_root`, `_cache_dir`, `_warm_path`, `_task_path`, `_lock_path`, `_with_flock`, `_seed_task_cache`, `_write_back_task_cache`. Pure functions/coroutines; unit-testable without touching `WorkspaceManager`.
3. **Wire helpers into `WorkspaceManager`** — extend `create_workspace`, `cleanup_workspace`, `prune_workspace`, `cleanup_stale_workspaces`. Preserve today's error-handling shape (`WorkspaceSetupError` still names the failing repo on git failure; new cache failures are warning-logged, never raised).
4. **Tests** — extend `tests/test_workspace.py` with the cases enumerated above. Existing tests continue to pass because `caches` defaults to `[]`.
5. **Docs + example config** — `docs/workspace.md`, `docs/config-schema.md`, `examples/linear.yaml`.
6. **Run the check gate** — `just lint && just test`. Both must pass before the PR is opened. Only the tests in `tests/test_workspace.py` and any config tests that touched `RepoCfg` change; every other test stays green.

## Edge cases / risks handled

- **First-ever run**: `warm/` doesn't exist. Helper creates an empty `warm/` dir and skips the copy — task starts with an empty (cold) cache, which is identical to today's behavior. Write-back on cleanup populates `warm/` for the next task.
- **Missing `.bare/` for an unrelated repo**: cache setup is per-repo; a repo without `caches:` triggers no cache work.
- **Orchestrator restart mid-task**: `create_workspace` is called again with the same `task_id`. Since `target.exists()` skips the `git worktree add`, and `_seed_task_cache` is idempotent (task_dir already exists → skip seed; symlink either already correct or replaced), the worktree comes back clean.
- **Replan (`reset_for_replan`)**: no-op on caches — task_dir survives the branch reset, so mypy on the second attempt keeps its warm cache. This is a small win beyond what today's design offers, and does not risk correctness (the cache is per-task).
- **`prune_workspace` after triage**: the pruned repo may have been touched by triage's lint. Write back its cache to warm before removing the worktree.
- **Cache path outside the worktree** (e.g. `../foo`): reject at config-load time. Add a validator on `CacheEntry.path` that rejects `..` segments and absolute paths — a cache entry must resolve strictly under the repo worktree root.
- **Very large caches** (e.g. nx caches can be hundreds of MB): `shutil.copytree` is O(n). Acceptable at task boundaries; both seed and write-back happen once per task, not per lint invocation. If this becomes a perf problem, a follow-up can switch to `cp --reflink=auto` on CoW filesystems.
- **Symlink target didn't exist yet** (`file` kind): touch `task_dir/<basename>` after seed so eslint's stat-before-write is happy.
- **Repo has a real `.mypy_cache/` at rest** (unlikely but possible): defensively delete it before creating the symlink. Log a warning if this happens — it means the repo committed a `.mypy_cache/`, which is a bug they should fix.
- **Stale `tasks/<task>/` dirs** (orchestrator crash between create and cleanup): `cleanup_stale_workspaces` now writes them back to warm before removal, so no cache work is lost.
- **File-kind cache with a tool that unlinks + creates** (some editors, some tools): if this becomes a problem we'll observe it as an orphaned target file inside `task_dir/` while the symlink is replaced by a regular file in the worktree. Write-back becomes a copy of the regular file — still correct, just less atomic. If tools start doing this we can revisit.

## Files touched

- `src/saga/config.py` — add `CacheKind`, `CacheEntry`, `RepoCfg.caches`, validator on `path`.
- `src/saga/services/git/workspace.py` — new helpers + additive changes to `create_workspace`, `cleanup_workspace`, `prune_workspace`, `cleanup_stale_workspaces`.
- `tests/test_workspace.py` — new cases enumerated above.
- `tests/test_config_phases.py` (or a new `tests/test_config_repos.py` if that file doesn't fit) — one test that `caches: []` is the default and one that a two-entry `caches:` list validates.
- `docs/workspace.md` — new section.
- `docs/config-schema.md` — extend the Repos section.
- `examples/linear.yaml` — add commented example.

No new runtime dependencies (`fcntl`, `shutil`, `asyncio`, `pathlib` are stdlib).

## Run/check commands

- Lint & types (per `justfile`): `just lint` → `uv run ruff check && uv run ruff format --check && uv run ty check`.
- Tests: `just test` → `uv run pytest`.
- Single-file test iteration during dev: `uv run pytest tests/test_workspace.py -x -q`.

No project-run needed in saga's own worktree for this ticket — the runtime behavior is exercised end-to-end only when saga runs against the real `olly`/`frontend` repos, which is a deploy-time check, not a local one.
