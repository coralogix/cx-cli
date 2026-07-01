# FORGE-187 — Remove Saga's dependency on the 'In Staging' status

## Goal

Coralogix is deleting the `In Staging` Linear workflow status. Saga currently references it by name in four places (preflight validation, terminal aggregation display, `_finalize_merged` board-write, `StatesCfg.staging`). When `In Staging` disappears, Saga will (a) refuse to start, (b) fail `write_state` on merge, and (c) mis-classify completed tickets as "not shipped" in the terminal summary. After this change, Saga must classify completion by Linear's `state_type == "completed"` and stop referencing a specific completed-state name anywhere.

## Current behavior (baseline)

Verified on the current branch, all 861 tests pass (baseline saved to `.saga/artifacts/before-tests.txt`).

- `services/linear/preflight.py:17` — the `("staging", "completed")` entry in `_STATE_DEFS` makes preflight raise `ConfigError` when `cfg.tracker.states.staging` (default `"In Staging"`) is absent from Linear. Saga won't start.
- `orchestrator/steps/terminal/aggregate.py:134-151` (`_acceptance_status`) — compares `terminal_status != staging_status`; anything not equal to `cfg.tracker.states.staging` gets `"unmet"`. If a workspace uses a different completed state name, all criteria show ❌ even on merged tickets.
- `orchestrator/steps/terminal/aggregate.py:215-275` (`_lookback`) — computes `shipped = terminal_status == staging and len(ts.prs) > 0` and branches its summary sentence on the same equality. Same mis-classification.
- `orchestrator/steps/review/pr_monitor.py:169-196` (`_finalize_merged`) — reads `staging = cfg.tracker.states.staging`, does `write_state(task.id, task.state, staging)`, then aggregates with `terminal_status=staging` and sets `last_processed_status=staging`. When `In Staging` is deleted, `write_state` raises `LinearMisconfigured` and the log noise is loud.
- `config.py:44` — `StatesCfg.staging: str = "In Staging"` is a required field with a default; existing configs that still declare `staging:` must continue to load (`StrictModel` has `extra="forbid"`).
- `_complete_cleanup` in `loop.py:478-613` (the FORGE-31 edge-triggered path) already uses `state_type in _TERMINAL_STATE_TYPES` correctly — no changes needed there.
- `routing.py` uses `state_type in _TERMINAL_TYPES` correctly — no changes needed.
- `tracker.py:298-304` (`active_column_names`) uses only the five active states (todo through in_review), does not reference `states.staging` — no changes needed.

## How to run / verify

CLAUDE.md is the source of truth: `just lint && just test` is the verification gate. Since Saga is a long-running orchestrator that requires Linear/GitHub credentials to run end-to-end, and this change is orchestration-loop plumbing, the unit test suite is the correct interface to exercise. Both before-state and after-state are captured through the pytest run.

- **Run tests:** `uv run pytest` (from `saga/`)
- **Focused tests:** `uv run pytest tests/test_terminal_aggregate.py tests/test_preflight.py tests/test_orchestrator_pr_review.py tests/test_cancel_cleanup.py tests/test_config_phases.py -q`
- **Lint + types:** `uv run ruff check && uv run ruff format --check && uv run ty check`
- **Artifacts:** save the before pytest transcript to `.saga/artifacts/before-tests.txt` (already captured) and the after transcript to `.saga/artifacts/after-tests.txt`.

## Implementation order (dependencies first)

Do the changes in this order so intermediate states compile:

### 1. `src/saga/orchestrator/steps/terminal/aggregate.py` — decouple aggregation from state name

The aggregator is the leaf: every caller passes into it. Change its API first so downstream callers can adopt it.

- **Add a `terminal_state_type: str` keyword-only parameter** to `aggregate_terminal`. Compute `is_completed = terminal_state_type == "completed"` once inside `aggregate_terminal` and pass a `bool` down.
- **Rewrite `_acceptance_status`** to take `is_completed: bool` instead of `terminal_status: str, staging_status: str`:
  - `if not is_completed: return "unmet"`
  - `if verdict_result == "pass" and not open_questions: return "addressed"`
  - `if verdict_result == "fail" or open_questions: return "unmet"`
  - `return "unknown"`
- **Rewrite `_acceptance_rows`** to take `is_completed: bool` (drop `terminal_status`, drop `cfg` — no longer needed for staging name lookup) and pass it through.
- **Rewrite `_lookback`** to take `is_completed: bool` instead of `terminal_status: str, cfg: Config`:
  - `shipped = is_completed and len(ts.prs) > 0`
  - Rewrite the sentence composition to branch on `is_completed` (was: `terminal_status == staging`):
    - `if is_completed: ...` (four sub-cases as today: no PRs / plan_superseded+missing / plan_superseded / missing / clean)
    - `else: ...` (canceled branch)
  - The header string still uses `terminal_status` (a display label), which is fine.
- **Update module docstring** (line 1-6): replace `merge → In Staging` with `merge → completed` and `Canceled` phrasing to talk about state types, not names.
- **Do not touch** the header `"📋 **Terminal Summary — {summary.terminal_status}**"` — the display should still show whatever Linear state name the ticket landed in.

### 2. `src/saga/orchestrator/steps/review/pr_monitor.py` — stop moving to a named staging state

- **Delete lines 173-178** (the `staging = self._cfg.tracker.states.staging` lookup, the `if task.state != staging` guard, and the `write_state(...)` call including its `try/except`).
- **Update the `aggregate_terminal` call** (lines 180-186) to pass `terminal_status=task.state, terminal_state_type="completed"`. Rationale: all PRs merged → the ticket is completed by definition, regardless of which state Linear's GitHub integration moves the card to. Using `task.state` as the display label is honest — it shows the current state at merge time.
- **Update the `update_state` call** (lines 187-189): change `last_processed_status=staging` to `last_processed_status=task.state`. Rationale: we haven't moved the card, so the watermark tracks the state we last acted on. Linear's card move will still be detected by `_maybe_terminal_cleanup` on the next tick (`state_name != last_processed_status`), which runs `_complete_cleanup` — idempotent via `aggregated_at`, so no double-post; just advances the watermark.
- **Update the docstring** on line 170: from `"...close the session, move to staging, aggregate..."` to `"...close the session, aggregate, clear PRs (Linear's GitHub integration moves the card to a completed state)."`.
- **Add an early-exit guard at the top of `poll()`** to prevent the "no PR to monitor" recovery/failure branch from firing on the next tick before Linear's card move lands:
  ```python
  async def poll(self, task: LinearTask) -> None:
      ts = await task_state_repo.get(task.id) or TaskState()
      if ts.pause is not None:
          return
      if ts.aggregated_at is not None:
          return  # already finalized; awaiting Linear's card move for _complete_cleanup
      prs = ts.prs
      ...
  ```
  Rationale: without this, tick N+1 sees the ticket still in `In Review`, prs empty, `_recover_prs_from_github` returns `[]` (it filters for `state="open"` PRs, and the PR is now merged), and `_mark_locally_failed` fires with "Reached review with no PR to monitor." — a false negative on a happily-merged ticket. `aggregated_at` is the durable "we're done here" marker.
- **`_finalize_merged` no longer needs `self._tracker`** for the removed write_state, but keep the constructor parameter unchanged (it may be used elsewhere in future; no scope creep).

### 3. `src/saga/services/linear/preflight.py` — stop requiring a named completed state

- **Remove `("staging", "completed")`** from `_STATE_DEFS` (line 17). The remaining five entries (todo/product_definition/technical_plan/implementation/in_review) still validate — these are the active columns Saga reads and writes.
- No comment change needed — the module docstring already talks generally about "required workflow states".

### 4. `src/saga/config.py` — mark `StatesCfg.staging` optional (do not remove)

- Change line 44 from:
  ```python
  staging: str = "In Staging"  # merge target; saga moves a merged PR's card here (PrMonitor)
  ```
  to:
  ```python
  staging: str | None = None  # deprecated; unused. kept optional so old YAML with `staging: "..."` still validates under extra="forbid".
  ```
- Rationale: making the field optional (typing `str | None`, default `None`) preserves backward compatibility for deployed configs that still carry `staging: "In Staging"` under `tracker.states`. Removing it entirely would break those configs at startup because `StrictModel` has `extra="forbid"`. Leaving it optional is safer and matches the ticket's "removed or made optional" language.
- No code path reads `cfg.tracker.states.staging` after steps 1-3, so the field is genuinely dead — but keeping it as an accepted-but-ignored key is intentional.

### 5. Update tests

The core behaviors change; the tests that assert on them need updates. All test edits are mechanical:

**`tests/test_preflight.py`**
- Update `_ALL_STATES` (line 26-33) to drop `"In Staging"` — preflight no longer requires it. If any test explicitly tests "missing staging state raises" (search shows there isn't one), it goes away.
- The `_make_cfg` helper's `staging: str = "In Staging"` default (line 56) becomes vestigial — either remove the parameter or leave it (it's passed to `StatesCfg(staging=...)` which now accepts str or None).

**`tests/test_terminal_aggregate.py`**
- The `aggregate_terminal(...)` call sites (25 of them) all need to add `terminal_state_type=...`:
  - When the test uses `terminal_status="In Staging"` → add `terminal_state_type="completed"`.
  - When the test uses `terminal_status="Canceled"` → add `terminal_state_type="canceled"`.
  - When the test uses `terminal_status="Done"` (only one, in `test_complete_cleanup_is_idempotent_via_aggregated_at`) → add `terminal_state_type="completed"`.
- The semantics of the tests do not change; they assert on the produced comment body, which continues to use `terminal_status` as the header. All the "In Staging" strings in assertions stay as-is.

**`tests/test_orchestrator_pr_review.py`**
- `test_poll_merged_pr_emits_terminal_aggregate` (line 643): change `assert after.last_processed_status == STATES.staging` to `assert after.last_processed_status == task.state` (or the literal `"In Review"` — the task fixture's default state).
- `test_poll_multi_pr_finalizes_when_all_merged` (line 714): same substitution.
- Neither test currently asserts a `write_state` call — the `_FakeBackend.write_state` recorder was there but the test only checked `last_processed_status`. After the fix, there'll be no `write_state` call from `_finalize_merged`; no assertion needs to be added, but if you want to be explicit, add `assert backend.state_writes == []` to lock in the removal.
- Add one new test (co-located): `test_poll_after_finalize_is_a_noop` — a `TaskState(aggregated_at=datetime.now(UTC))` with `task.state == "In Review"` and empty `prs`; assert `poll()` returns without calling `_mark_locally_failed` (i.e. `pause` stays `None`, no friction records added). This locks in the early-exit guard.

**`tests/test_cancel_cleanup.py`**
- `_complete_cleanup` call sites (many) don't change — they already pass `state_name` to `_complete_cleanup`, which then calls `aggregate_terminal` internally. The internal call is what changes signature; test assertions on comment body / watermark stay the same.

**`tests/test_config_phases.py`**
- `test_config_does_not_require_states` (line 33): change `assert cfg.tracker.states.staging == "In Staging"` to `assert cfg.tracker.states.staging is None`.
- `test_states_cfg_unknown_field_rejected` (line 148): the payload includes `"staging": "S"` — that still validates fine (str is accepted by `str | None`). The rejection is on the `"extra"` key. Test passes unchanged.

### 6. Loop-side callers of `aggregate_terminal` in `loop.py`

- **`_cancel_cleanup`** (line 525): add `terminal_state_type="canceled"`.
- **`_complete_cleanup`** (line 588): add `terminal_state_type="completed"`.
- **`terminal/step.py`** (line 23): add `terminal_state_type=ctx.task.state_type` — the task's live state type; the aggregation step reads it from the task passed in.

### 7. Doc touch-ups (small, in-scope)

- **`docs/flow.md`** line 21, 72, 92: change the phrasing so it doesn't imply Saga writes to `In Staging`. Suggest: `| Terminal statuses (e.g. Done, In Staging) | Linear type "completed" — Saga detects via state_type and runs terminal aggregation. |` and step 6 becomes "PR merged → Linear moves the card to a completed status; Saga runs terminal aggregation on the merge edge."
- **`docs/config-schema.md`** lines 60, 93: mark `staging` as deprecated in the table.
- **`examples/linear.yaml`** lines 4, 38: drop `staging: "In Staging"` from the example `states:` block, and update the header comment's status flow to end at `In Review`.
- Leave the design docs under `docs/pipeline-product-definition/` and `docs/status-phase-sync-plan.md` untouched — those describe historical design; edits are out of scope for a code-behavior ticket.

## Edge cases / risks

- **Idempotency of the completion path**: after `_finalize_merged` sets `aggregated_at` and clears prs, on the next tick — if Linear has NOT yet moved the card — `poll()` runs and now early-exits on `aggregated_at`. When Linear moves the card, the ticket disappears from `list_tasks()`, `_maybe_terminal_cleanup` sees `("<something>", "completed")`, watermark differs from `last_processed_status=task.state ("In Review")`, so `_complete_cleanup` runs: `aggregate_terminal` returns False (marker already set) → no double-comment; watermark advances; state is idempotently re-cleaned. Verified against loop.py:493-498 and aggregate.py:449-452.
- **Existing YAML configs**: keeping `staging: str | None = None` means configs with `staging: "In Staging"` continue to validate under `extra="forbid"`. Configs without it get `None`. No deployment breaks.
- **Preflight coverage of the five remaining states**: still fatal on any of the active states missing. Consistent with existing docs (`docs/flow.md:92`).
- **No new state-type reads from tracker**: `_finalize_merged` and callers already have `task.state_type` on the `LinearTask`; no extra Linear API calls.
- **Test suite scope**: 861 tests currently pass. Estimate ~30 test assertion updates in `test_terminal_aggregate.py`, ~2 in `test_orchestrator_pr_review.py`, ~1-2 in `test_preflight.py`, ~1 in `test_config_phases.py`, plus one new test in `test_orchestrator_pr_review.py`. No new mocks or fixtures required.

## Verification checklist (after implementation)

1. `uv run ruff check && uv run ruff format --check && uv run ty check` — clean.
2. `uv run pytest` — all 861+ tests pass (the new test brings it to 862).
3. Manual grep audit: `rg -n 'states\.staging|cfg\.tracker\.states\.staging|"In Staging"' src/` — should return zero matches in `src/` (tests still contain the literal for header assertions and legacy state names, which is fine).
4. Manual grep audit: `rg -n 'write_state.*staging' src/` — zero matches.
5. Capture the after-state pytest transcript to `.saga/artifacts/after-tests.txt` for the PR artifact.

## Out of scope (per ticket)

- Adding a replacement "completed" state to Linear.
- Changes to `routing.py`, `loop.py._complete_cleanup`, or `tracker.py:298-304` — they already use `state_type` correctly.
- Removing `staging` from `StatesCfg` entirely (chose the optional-with-default-None migration path instead).
- Rewrites of `docs/pipeline-product-definition/*` or `docs/status-phase-sync-plan.md`.
