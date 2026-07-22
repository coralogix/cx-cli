# FORGE-509 — Skip local dev-server run for cx-web-workspace; gate for manual UI check

## Goal
For tasks scoped to the `frontend` repo (`cx-web-workspace`), the `technical_plan` and `implementation` steps must NOT bring up the local dev server (`pnpm nx serve` / `dstaging` / `dprod`) as part of the shared `feedback_loop` run-and-observe. Code/lint/tests proceed normally. After implementation, raise a `pause=GATE` human checkpoint asking for manual UI verification. Other repos are unchanged.

## Current behavior (baseline, grounded in code)
- `src/saga/orchestrator/steps/_shared/feedback_loop.md` unconditionally tells the agent to discover a "Run command" and bring up the interface. It is inlined statically by `read_and_expand_prompt` (`src/saga/services/claude/prompts.py:37-62`); there is **no** config/repo awareness at expansion time.
- `feedback_loop.md` is pulled into `technical_plan.md:11` and `implementation.md:16` via `{{shared:feedback_loop}}`.
- `RepoCfg` (`src/saga/config.py:49-57`) has no per-repo run flag.
- Implementation's `gate()` is inherited from `Step.gate()` (`step.py:213-222`) → returns `NONE` for frontend, so it auto-advances to `pr_review`.
- The frontend dev server itself cannot be run in this worktree (out of scope; running it is the reported crash). Baseline is established by code inspection above.

## Run / check commands (saga repo)
- Install: `uv sync`
- Lint + types: `just lint` (ruff + ty); autofix `just lint-fix`
- Tests: `just test`; scoped: `uv run pytest tests/test_config_repos.py tests/test_prompts.py tests/test_technical_plan.py tests/test_implementation_step.py`
- `.claude/skills/code-checks` is the canonical verify path.

## Design
The shared fragment is expanded statically, so the skip is delivered by **injecting a repo-scoped override directive** into the two step prompts when an in-scope repo carries a new per-repo flag — `feedback_loop.md` itself is left unchanged (keeps other repos byte-identical, success criterion 4). The human checkpoint reuses the existing `pause=GATE` machinery via an `implementation` `gate()` override.

### Change order (dependencies first)

**1. `src/saga/config.py` — add per-repo flag**
- Add to `RepoCfg` (after `caches`, line 54): `skip_local_run: bool = False` with a one-line *why* comment (repos whose local dev server crashes the saga task flow — cx-web-workspace, FORGE-509 — skip feedback-loop run-and-observe and gate implementation for manual UI verification). `StrictModel`/`extra="forbid"` already covers validation.

**2. `deploy/config.yaml` — enable for frontend**
- Under `frontend:` (lines 17-24) add `skip_local_run: true`.

**3. `src/saga/orchestrator/steps/generic.py` — shared helpers**
Add module-level helpers (imported by both steps; keeps `technical_plan` from importing `implementation`):
- `def in_scope_repo_keys(cfg: Config, ts: TaskState) -> list[str]` — triaged repos (`ts.triage.repos`) filtered to configured keys, else all `cfg.repos`. (This is identical to `implementation._target_repo_keys`; optionally refactor `implementation.py` to import this and drop its local copy to avoid drift — optional cleanup, keep behavior identical.)
- `def skip_local_run_repos(cfg, ts) -> list[str]` — the `github` slugs of in-scope repos whose `skip_local_run` is True.
- `def local_run_skip_note(cfg, ts) -> str | None` — `None` when `skip_local_run_repos` is empty; otherwise a clearly-marked override section, e.g.:
  > `## Override: do not run the local dev server`  — For these repos the local dev server (`pnpm nx serve` / `pnpm run dstaging` / `pnpm run dprod`) is known to crash the saga task flow (FORGE-509): `<slugs>`. **Skip** the "Run command" discovery and the run-and-observe portion of the feedback loop for these repos. Still run lint / typecheck / tests. A human will verify the UI manually after the change is ready.

**4. `src/saga/services/claude/prompts.py` — implementation prompt seam**
- Add optional param `local_run_note: str | None = None` to `build_implementation_prompt`. When non-None, append it as a final section **after** `instructions` (last position → strongest instruction-following for an override). Omit entirely when None (non-frontend prompts unchanged).

**5. `src/saga/orchestrator/steps/technical_plan/__init__.py` — inject into plan prompt**
- Override `extra_context(self, ctx)` (currently inherited `None`) to return `local_run_skip_note(ctx.cfg, ctx.ts)` (sync; uses the dispatch snapshot `ctx.ts`, which has `triage.repos` populated since triage ran earlier). `extra_context` output is appended to the task-context section (`step.py:405-407`). Keep prior behavior (return `None`) when the note is empty.

**6. `src/saga/orchestrator/steps/implementation/__init__.py` — inject note + gate for UI check**
- In `work()`: after `build_implementation_prompt(...)`, pass `local_run_note=local_run_skip_note(ctx.cfg, ts)` (ts already read at line 297).
- Add `gate()` override:
  ```
  async def gate(self, ctx, verdict):
      level = await super().gate(ctx, verdict)
      if level is GateLevel.APPROVE:
          return level
      ts = await task_state_repo.get(ctx.task.id) or TaskState()
      if skip_local_run_repos(ctx.cfg, ts):
          return GateLevel.APPROVE
      return level
  ```
- In `post_step()`: when `skip_local_run_repos(ctx.cfg, ts)` is non-empty, augment the `summary` (currently `Code on branch ...`, line 482) to add "Please verify the UI manually before approving." so the `pause=GATE` outcome comment/Slack (`generic._outcome_comment` includes `out.summary`) states the ask. `super().post_step` (`step.py:326-334`) then runs `gate()` → APPROVE → `pause=GATE`. Session stays open (review-enabled path keeps it), so on human Approve `_unblock` → `advance()` resumes into `pr_review`.

**7. Docs**
- `docs/config-schema.md`: document `skip_local_run` under the repo section (near `caches`, lines 44-57).

## Edge cases / risks
- **Snapshot scoping in `technical_plan.extra_context`:** uses `ctx.ts` (dispatch snapshot). Triage runs before technical_plan so `triage.repos` is normally present. If `triage` is absent, `in_scope_repo_keys` falls back to all configured repos, so the directive would fire whenever `frontend` is configured — a harmless over-trigger (the directive is a conditional "for these repos" instruction). Acceptable; note in the comment. Implementation reads fresh `ts`, so it is precisely scoped.
- **Double-gate:** if `technical_plan`/`implementation` are already in `gates.mandatory_approve` or hit `risk_levels`, `super().gate()` returns APPROVE first and the override is a no-op — no double pause.
- **Review disabled (`pr_monitor` None):** gate still pauses; on approve `advance()` moves to the next phase. Acceptable.
- **Do not edit `feedback_loop.md`** — keeps other repos' prompts identical (criterion 4).
- Branch/PR creation is unaffected: the change only removes a run instruction and adds a post-PR gate; `open_pr` / `git/workspace.py` paths are untouched.

## Verification
1. `just lint && just test` green.
2. New/updated tests:
   - `tests/test_config_repos.py`: `skip_local_run` defaults `False`; accepts `true`; unknown field still rejected.
   - `tests/test_prompts.py`: `build_implementation_prompt` includes the note text when `local_run_note` passed; excludes it (and any run-override wording) when `None`.
   - Helper test (in a suitable module, e.g. extend `test_flow_generic.py` or a new `tests/test_generic_helpers.py`): `local_run_skip_note` returns a directive containing `coralogix/cx-web-workspace` when the flagged repo is in scope; `None` otherwise; respects `ts.triage.repos` scoping (no note when only non-flagged repos triaged).
   - `tests/test_technical_plan.py`: `extra_context` returns the directive when a flagged repo is in the snapshot's triaged repos; `None` when not.
   - `tests/test_implementation_step.py`: `gate()` returns `GateLevel.APPROVE` (→ `pause=GATE`) when a flagged repo is in scope; `NONE` for a non-flagged repo; `post_step` summary contains the manual-UI ask for the flagged case.
3. Behavioral check: for a synthesized frontend-scoped `ctx`, confirm the technical_plan and implementation prompts contain the "do not run the local dev server" override; for a non-frontend repo, confirm the prompts are unchanged (no override section) and gate stays `NONE`.
4. Frontend dev server is intentionally not run here (out of scope; that run is the reported crash) — the guard is verified by prompt/gate assertions, not by launching the Angular/Nx server.