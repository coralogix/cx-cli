## FORGE-20 — Surface real errors, and make pause→resume re-enter the step cleanly

Two independent fixes in two files, plus tests. The bug is fully reproducible by inspection of the code paths cited in the ticket.

### Run / check commands (from `saga/`)
- Tests: `just test` (or `uv run pytest tests/test_flow_runner.py tests/test_orchestrator_session_lifecycle.py` to focus)
- Lint + types: `just lint`
- The runtime entry point is `just run`, but reproducing the bug end-to-end requires Linear / Slack / GitHub credentials and a live ticket — not available in this worktree. Both bugs are unit-testable in the existing pytest fixtures; that is how we will verify.

### Reproduction (before-state)
- **Bug 1 (vague error):** `runner.py` lines 52–54 catch `except Exception:` and pass the literal string `f"`{step.name}` raised an error."` into `mark_locally_failed`. The string discards the caught exception's type, message, and traceback — confirmed by `tests/test_flow_runner.py::test_run_work_raises_marks_locally_failed`, which asserts only `mark_failed.assert_awaited_once_with("issue-1", ANY)`. The Linear comment posted via `needs_human()` therefore just says ```step raised an error.```
- **Bug 2 (pause→resume not atomic):** `loop.py::_unblock` (lines 325–345) writes `{"pause": None, "consecutive_failures": {}}` (plus optional `prs`) but never touches `stage`. A step that failed during `work()` was at `Stage.WORKING` when `mark_locally_failed` flipped `pause` to `FAILED`. When a human removes the `needs-human` label, `_reconcile` (line 394) calls `_unblock`, which clears `pause` but leaves `stage = WORKING`. The next dispatch's `StepRunner._drive` reads `stage is Stage.WORKING` (runner.py:56–59) and **skips `pre_step`**, re-entering `work()` directly. This is the silent failure the ticket asks us to fix — `pre_step` (which loads fresh context, e.g. re-reads the ticket for the re-run) never runs.

The verifier-failure path (`generic.py:185–190`) and retry-exhausted path (`generic.py:149–153`) already fold `verdict.notes` / `out.summary` into the escalation reason, so they meet criterion 1 already; the gap is only the bare-exception path in `runner.py:54`. The ticket calls this out explicitly.

---

### Changes

#### 1) `src/saga/orchestrator/steps/runner.py` — capture exception detail on the catch-all

Bind the exception and format `<TypeName>: <message>` (plus a short traceback) into the reason passed to `mark_locally_failed`.

```python
# at top of file
import traceback

# replace the except block at runner.py:52–54
except Exception as exc:
    logger.exception(f"step execution error task={task.id} step={step.name}")
    detail = _format_exception(exc)
    await self._deps.mark_locally_failed(
        task.id,
        f"`{step.name}` raised an error.\n\n```\n{detail}\n```",
    )
```

Add a private helper next to the class (kept small and easy to test):

```python
def _format_exception(exc: BaseException, *, max_chars: int = 1500) -> str:
    """Format `<Type>: <message>` plus a short traceback for surfacing in Linear/Slack.

    Truncated to keep Linear comments readable when a stack frame includes long paths.
    """
    head = f"{type(exc).__name__}: {exc}".strip() or type(exc).__name__
    tb = "".join(traceback.format_exception(exc)).strip()
    if len(tb) <= max_chars:
        return tb
    return f"{head}\n…(traceback truncated)…\n{tb[-max_chars:]}"
```

Rationale:
- `traceback.format_exception(exc)` (single-arg form, Python 3.10+) returns the type+message+traceback in one call. Saga is Python 3.13+ (per CLAUDE.md), so this is safe.
- The reason ends up inside a markdown comment (`needs_human` posts `🙋 **Needs human** — {reason}`). A fenced code block keeps the traceback monospaced and prevents Slack/Linear markdown from mangling it.
- We truncate from the head and keep the tail because the most useful frames are usually the innermost ones; the head line is preserved at the top so the exception type/message is always visible even when truncated.

No change is needed in `generic.py` — criteria 1's sub-bullets for `on_failure` and `record_verifier_fail` are already satisfied (verified by reading lines 149–153 and 185–190). The plan documents them so the implementation step doesn't try to "improve" working code.

#### 2) `src/saga/orchestrator/loop.py::_unblock` — reset `stage` so the step re-enters via `pre_step`

```python
# replace the updates dict at loop.py:341
updates: dict[str, Any] = {"pause": None, "stage": None, "consecutive_failures": {}}
```

Passing `stage=None` is the established idiom (see `generic.advance` line 107 which does the same). `TaskState._migrate_legacy` (`schemas/state.py:260–262`) pops the `None` key before validation, so the field falls back to its default `Stage.ENTERED`. After this, the very next `StepRunner.run` for the task sees `ts.stage is Stage.ENTERED` and routes through `pre_step` → `set_stage(WORKING)` → `work()`, which is the atomic resume the ticket describes.

Update the docstring comment to record the new invariant:

```python
async def _unblock(self, task: LinearTask, ts: TaskState) -> None:
    """Resume a paused ticket: ``GATE`` advances to the next step; any other pause
    clears the block, resets ``stage`` to ENTERED so the next dispatch re-runs
    ``pre_step``, and resets retry budgets so the current step re-runs cleanly."""
```

**Why this is safe for every pause that flows through `_unblock`:**

| Pause | Stage at pause time | After fix |
|---|---|---|
| `NEEDS_INPUT` | already `ENTERED` (set by `pause_for_input`, called from runner.py:70 right after pre_step returns PAUSE) | still `ENTERED` — no behavior change |
| `FAILED` (from `on_failure`) | usually `WORKING` (failed mid-work) | reset to `ENTERED` — pre_step re-runs ✅ |
| `FAILED` (from runner.py:54 catch-all) | `ENTERED` or `WORKING` depending on where the throw happened | reset to `ENTERED` ✅ |
| `STOPPED` | varies; resume only via `approve()` | reset to `ENTERED` — pre_step re-runs (matches "resume = re-run current step" in `docs/status-phase-sync-plan.md` §3) ✅ |
| `GATE` | unused: `_unblock` short-circuits to `advance()` before the `updates` dict | no change (`advance` already clears stage at generic.py:107) |

**Note on `consume_thread_reply` (loop.py:295):** that path deliberately sets `stage=Stage.WORKING` because a Slack thread reply is treated as "I'm answering your question; continue work without re-running pre_step." This is intentional and out of scope — we do not change it. The fix applies only to the *label-removal* resume path, which is what the ticket calls out.

---

### Order of changes
1. Fix `runner.py` exception capture + helper (independent).
2. Update `runner.py` test asserting the reason includes the exception detail.
3. Fix `_unblock` to add `"stage": None`.
4. Update the existing `_unblock` tests + docstring; add a new test asserting `stage` reset.

Steps 1–2 and 3–4 are independent and could be done in either order; the implementation step should still keep them in separate commits / sections for review clarity.

### Tests to add / update

**`tests/test_flow_runner.py`**

- Tighten `test_run_work_raises_marks_locally_failed` (line 386–400): replace `ANY` with an assertion that the reason
  - contains the step name in backticks,
  - contains the exception type name (`RuntimeError`),
  - contains the exception message (`boom`).
- Optionally add a sibling test using a custom exception class with a multiline message to confirm the traceback formatting / truncation behaves (assert overall length ≤ ~1700 chars and that the type name appears).
- Apply the same reason-content tightening to `test_run_create_workspace_raises_marks_locally_failed` (line 372–383).

**`tests/test_orchestrator_session_lifecycle.py`**

- Extend `test_approve_failure_clears_block_for_retry` (line 483–501): set `task_states["issue-1"] = TaskState(pause=Pause.FAILED, stage=Stage.WORKING, …)` in the arrange block and add `assert after.stage is Stage.ENTERED` to the assertions.
- Add `test_unblock_via_label_removal_resets_stage_to_entered`: arrange a `Pause.FAILED` + `stage=Stage.WORKING` task with the `Needs Human` label absent (already removed), drive `_reconcile`, assert `after.stage is Stage.ENTERED` and `after.pause is None`.
- Add `test_unblock_does_not_clobber_entered_stage_for_needs_input`: arrange `Pause.NEEDS_INPUT` + default `stage=Stage.ENTERED`, drive `_reconcile`, confirm `stage` stays `ENTERED` (regression guard against later code accidentally writing `WORKING`).

These three additions, plus the runner test tightening, fully exercise the two success criteria from the ticket.

### Edge cases / risks
- `_unblock` is called from both `approve()` and `_reconcile()` — fixing it once covers both surfaces (Slack Approve button / `saga approve` CLI **and** human deletes the label in Linear). No additional call-site edits needed.
- The `consume_thread_reply` path keeps writing `Stage.WORKING`; we explicitly leave it alone (Slack reply ≠ label removal, different semantics). Document this in the new test's docstring so future readers don't try to "unify" the two.
- The `STOPPED` pause now also gets `stage` reset by `_unblock`. STOPPED only resumes through explicit `approve()` (see `_reconcile` line 388–395 which excludes STOPPED from the label-removal branch); the behavior change is "the next dispatch re-runs `pre_step` instead of jumping into `work()`" which matches docs/status-phase-sync-plan.md §3 ("resume = re-run current step"). This is a desirable side-effect, not a regression.
- Traceback truncation: 1500 chars keeps long Linear-issue comments readable; the head line (`<Type>: <message>`) is always preserved so the truncation can never strip the most important information.
- `traceback.format_exception(exc)` returns a `list[str]`; we `"".join` it. Tested in Python 3.13.
- No persisted-comment schema changes; no migration risk.

### Verification (after-state)
1. `just lint` — clean.
2. `just test` — all tests green, including the new ones.
3. Targeted check of the runner test that previously asserted `ANY` now asserts the exception detail is present in the reason string.
4. Inspect a manual trace by reading the new `_format_exception` helper output for a `RuntimeError("boom")` — it should produce something like:
   ```
   Traceback (most recent call last):
     File ".../runner.py", line N, in run
       …
   RuntimeError: boom
   ```
   which gets wrapped as `` `step_name` raised an error.\n\n```\n<above>\n``` `` in the Linear comment.
5. Confirm `_unblock` post-state: in the new test, `task_states["issue-1"].stage is Stage.ENTERED` after label removal.

No artifacts (videos/screenshots/CLI traces) are produced for this change — it is a pure unit-level behavior fix verified by pytest. The "before/after" evidence is the test diff itself plus the asserted reason content.
