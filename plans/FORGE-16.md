# FORGE-16 — Implementation plan: unrecoverable prerequisite failures must not retry

## Goal recap

When `ImplementationStep.work()` detects there is no approved plan in `TaskState`, the
current behaviour returns `WorkStatus.FAILED`, which the runner routes through
`on_failure()` — bumping `consecutive_failures[implementation]` and only escalating to
`needs_human` once the counter hits `max_attempts() == 3`. We want that single, deterministic
"no plan" check to fail-fast: flag `needs-human` on the first attempt, leave the per-step
retry counter at 0, and post a `🙋 Needs human — …` comment immediately.

We implement **Option A** from the ticket: add an `unrecoverable: bool = False` field to
`StepOutcome`, and special-case it in `on_failure`. The flag is a generic seam — other
steps with hard prerequisites can opt in later.

## Before-state (observed from the worktree, no run required)

The orchestrator is a long-lived process that polls Linear (`LINEAR_OAUTH_TOKEN` required)
and dispatches agents in isolated worktrees. There is no path to exercise the
implementation step end-to-end inside this sandbox without Linear credentials and a real
Claude Code session. The current behaviour is therefore observed from the code:

- `src/saga/orchestrator/steps/implementation/__init__.py:199-204` —
  no `plan_text` → `StepOutcome(status=WorkStatus.FAILED, summary="No approved plan in task state — refusing to implement without a plan.")`.
- `src/saga/orchestrator/steps/runner.py:79-81` — `WorkStatus.FAILED` → `on_failure(ctx, step, out)`.
- `src/saga/orchestrator/steps/generic.py:123-153` — `on_failure` always bumps
  `consecutive_failures[step.name]`, appends an `AGENT_ERROR` `StepRecord`, publishes
  transcript, and only calls `mark_locally_failed` once `attempts >= step.max_attempts()`
  (default 3). So the no-plan branch consumes 3 retries.
- `tests/test_implementation_step.py:430-444` (`test_work_fails_fast_without_a_plan`) only
  asserts `out.status is WorkStatus.FAILED`; it does not currently assert anything about
  the retry behaviour, so the bug above is invisible in unit tests.

Run/check commands (from `CLAUDE.md` and `justfile`):
- Tests: `just test` (uses the in-process `task_states` fixture; no Linear needed).
- Lint + types: `just lint` (`ruff check`, `ruff format --check`, `ty check`).
- Auto-fix: `just lint-fix`.
- Full pre-commit gate (`.claude/skills/code-checks/SKILL.md`): `just lint-fix && just lint && just test`.

## Design — Option A: `StepOutcome.unrecoverable`

### 1. Schema change — `src/saga/schemas/step.py`

Add a single new field to `StepOutcome`:

```python
class StepOutcome(BaseModel):
    status: WorkStatus
    session_id: str | None = None
    summary: str | None = None
    detail: StepDetail = StepDetail()
    metrics: StepMetrics | None = None
    verdict: Verdict | None = None
    gate: GateLevel | None = None
    unrecoverable: bool = False  # FAILED + unrecoverable → on_failure escalates immediately, no retry
```

Default `False` keeps every existing FAILED outcome on the current retry path. The flag
is only meaningful when `status is WorkStatus.FAILED`; we don't enforce that with a
validator (a one-line invariant; future readers can spot it from the docstring) — the
runner only consults the field after seeing FAILED.

### 2. Engine change — `src/saga/orchestrator/steps/generic.py` `on_failure`

Add an early branch before the retry/counter logic. When the outcome is unrecoverable:

- Do **not** read or bump `consecutive_failures[step.name]`.
- Append a `StepRecord` with `failure_class=FailureClass.NEEDS_INPUT` (the existing class
  for "step paused on ambiguity / blocking question"; the no-plan case is exactly that)
  and **no** `attempt` value (since this is not a retry).
- Skip `session_transcript.publish_for_step` — an unrecoverable prerequisite failure has
  no agent session worth surfacing (the no-plan branch returns before any agent turn).
- Call `ctx.deps.mark_locally_failed(ctx.task.id, reason)` immediately. `reason` is
  `out.summary` if present, else a generic fallback (`f"`{step.name}` failed with an
  unrecoverable error."`). `mark_locally_failed` is the canonical "saga is stuck" entry
  point: it sets `Pause.FAILED`, suppresses the flag for already-abandoned tickets, and
  funnels through `needs_human()` to post the `🙋 Needs human — …` comment and Slack
  ping. No new wiring is needed.

Updated docstring on `on_failure` to mention the unrecoverable short-circuit.

```python
async def on_failure(ctx, step, out=None):
    if out is not None and out.unrecoverable:
        ts = await task_state_repo.get(ctx.task.id) or TaskState()
        record = StepRecord(
            step=step.name,
            at=datetime.now(tz=UTC),
            status=WorkStatus.FAILED,
            failure_class=FailureClass.NEEDS_INPUT,
            summary=out.summary,
        )
        await task_state_repo.update_state(
            ctx.task.id, step_records=[*ts.step_records, record]
        )
        reason = out.summary or f"`{step.name}` failed with an unrecoverable error."
        logger.warning(
            f"step failed unrecoverably; escalating immediately task={ctx.task.id} step={step.name}"
        )
        await ctx.deps.mark_locally_failed(ctx.task.id, reason)
        return
    # ... existing retry/counter path unchanged
```

**Why use `FailureClass.NEEDS_INPUT` and not a new class?** The taxonomy already has a
"step paused on a blocking question" entry. Adding a new class for "missing prerequisite"
would be over-fitting; `NEEDS_INPUT` is the right semantic and is already what
`pause_for_input` records. Re-using it keeps the taxonomy stable and means existing
consumers (e.g. `format_prior_attempts`, which today only branches on `AGENT_ERROR` /
`VERIFY_REJECT`) silently ignore it — which is correct: an unrecoverable record is not a
retry hint to inject into a later prompt, because there will not be a retry.

### 3. Step change — `src/saga/orchestrator/steps/implementation/__init__.py`

Set the flag on the existing no-plan guard:

```python
if not ts.plan_text:
    logger.error(f"implementation kickoff has no approved plan in state task={task.id}")
    return StepOutcome(
        status=WorkStatus.FAILED,
        unrecoverable=True,
        summary="No approved plan in task state — refusing to implement without a plan.",
    )
```

No other call site in the file changes. The other `WorkStatus.FAILED` returns in `work()`
(missing workdir, missing session, agent turn ended `event="failed"`) **stay on the retry
path** — they are transient and re-running the step is a legitimate recovery action. Only
the missing-plan deadlock is unrecoverable: until a human adds a plan or moves the ticket
back to Technical Plan, re-running implementation will hit the same guard.

### 4. Tests

#### 4a. `tests/test_implementation_step.py`

Tighten the existing `test_work_fails_fast_without_a_plan` (line 430) to assert the new
contract:

```python
assert out.status is WorkStatus.FAILED
assert out.unrecoverable is True
assert "No approved plan" in (out.summary or "")
```

#### 4b. `tests/test_flow_generic.py` — new tests under the existing "on_failure" section

Add three tests that exercise the new branch in `on_failure`:

1. `test_on_failure_unrecoverable_escalates_immediately_no_counter_bump` — `out.unrecoverable=True`,
   `max_attempts=3`. After a single `on_failure` call:
   - `task_states["issue-1"].consecutive_failures == {}` (no entry created).
   - `mark_failed` is awaited once with the outcome's `summary` as the reason.
   - A `StepRecord` is appended with `failure_class=FailureClass.NEEDS_INPUT`,
     `attempt is None`, and the summary preserved.

2. `test_on_failure_unrecoverable_uses_fallback_reason_without_summary` — `unrecoverable=True`
   but no `summary`. The escalation reason falls back to
   `"`<step>` failed with an unrecoverable error."`.

3. `test_on_failure_unrecoverable_does_not_publish_transcript` — patch
   `session_transcript.publish_for_step` and assert it is not called on the unrecoverable
   branch. (Optional but cheap; documents intent.)

Regression check: the existing
`test_on_failure_max_attempts_1_escalates_first_time` / `_3_escalates_on_third` /
`_summary_included_in_escalation_reason` / `_appends_step_attempt_error` tests must
continue to pass unchanged — they all call `on_failure` with `unrecoverable=False`
(default), so the existing path is untouched.

## Order of changes (dependencies first)

1. **`src/saga/schemas/step.py`** — add the `unrecoverable` field. Nothing depends on
   anything new here; the field defaults to `False` so this commit alone is a no-op.
2. **`src/saga/orchestrator/steps/generic.py`** — branch in `on_failure`. The schema
   change is the only prerequisite.
3. **`src/saga/orchestrator/steps/implementation/__init__.py`** — set `unrecoverable=True`
   on the no-plan branch.
4. **`tests/test_implementation_step.py`** — strengthen `test_work_fails_fast_without_a_plan`.
5. **`tests/test_flow_generic.py`** — add the three `on_failure` tests above.

The whole change is small (one field, one branch, one call site, three new tests, one
test tightening). It can land as a single commit.

## Edge cases & risks

- **Other FAILED returns in `implementation.work()`** (no workdir, no session,
  `outcome.event == "failed"`) intentionally stay on the retry path. They are transient
  (worktree setup race, session manager hiccup, transient agent error) and a retry can
  legitimately fix them. Only the missing-plan guard is unrecoverable.
- **Mid-step state mutation.** The current `on_failure` re-reads `TaskState` before
  bumping the counter to avoid clobbering a concurrent write (e.g. an MCP tool persisting
  result during the turn). The unrecoverable branch also re-reads `ts` for the same
  reason before appending the `StepRecord`. We never write the counter, so there is no
  TOCTOU issue with `consecutive_failures`.
- **`mark_locally_failed` short-circuit on abandoned tickets.** If a human has already
  moved the ticket to a Cancelled/Done/Backlog state, `mark_locally_failed` already
  refuses to apply the needs-human label and instead persists `Pause.STOPPED`. The new
  branch inherits this behaviour automatically — no extra guard needed.
- **`Pause` field is set by `mark_locally_failed`, not by `on_failure`.** That keeps the
  existing contract intact: `on_failure` either escalates (which sets `Pause.FAILED` via
  `mark_locally_failed`) or does nothing visible at the pause level (still retrying).
- **No risk to `record_verifier_fail`.** It's a separate code path with its own counter
  and `loop_back`; it does not read `StepOutcome.unrecoverable`. The verifier "FAIL"
  semantic (the change is wrong, try again) is fundamentally different from the
  unrecoverable semantic (we cannot try at all).
- **`format_prior_attempts` integration.** It already filters on `AGENT_ERROR` and
  `VERIFY_REJECT`; `NEEDS_INPUT` records are silently ignored, which is what we want — an
  unrecoverable record carries no useful "lesson" for a future retry, because there will
  not be a future retry of the same dispatch.

## Verification

### Automated

From the repo root:

```bash
just lint-fix && just lint && just test
```

All three must pass. The relevant tests:

- `tests/test_implementation_step.py::test_work_fails_fast_without_a_plan` (tightened) —
  asserts `out.unrecoverable is True`.
- `tests/test_flow_generic.py::test_on_failure_unrecoverable_*` (new, 3 tests) — assert
  the immediate-escalate / no-counter-bump / fallback-reason / no-transcript contract.
- All existing `test_on_failure_*` and `test_record_verifier_fail_*` tests — regression
  guard that the unchanged retry path is intact.

### Behaviour to observe

**Before:** Run orchestrator on a ticket in Implementation phase with `TaskState.plan_text=None`.
The agent turn does not run; `work()` returns FAILED; the runner calls `on_failure` and bumps
`consecutive_failures["implementation"]` to 1. The Linear card stays in Implementation;
on the next tick the same thing happens again. After three ticks the task is finally
flagged "Needs human" with a comment that says
`` `implementation` failed after 3 attempts. No approved plan in task state — refusing to implement without a plan. ``

**After:** Same starting condition. The agent turn does not run; `work()` returns FAILED
with `unrecoverable=True`; the runner calls `on_failure`, which bypasses the counter
entirely; `consecutive_failures["implementation"]` is **0**; `mark_locally_failed` runs
immediately and posts
`🙋 **Needs human** — No approved plan in task state — refusing to implement without a plan.`
on the very first tick.

### Real-ticket verification (per ticket "Success criteria 2")

Once merged, on a Linear ticket in Implementation phase with the plan deliberately
cleared from state (`plan_text=None` in the marked JSON state comment), run the
orchestrator. Expected:

- `🙋 **Needs human** — No approved plan…` posted on the first tick.
- `consecutive_failures["implementation"] == 0` in the persisted state.
- `step_records` includes one entry for `implementation` with
  `failure_class == NEEDS_INPUT` and no `attempt` value.
- The Slack thread receives one `on_needs_human` ping, not three.

This step is performed by the assignee after the change merges; the sandbox does not have
Linear credentials.

## Out of scope (per ticket)

- No changes to other prerequisite checks (`out-of-scope PR`, `no PR after work`).
- No changes to verifier/gate behaviour.
- No retry-counter or `max_attempts` refactor.
- No changes to how `needs_human` notifications render in Linear/Slack.
