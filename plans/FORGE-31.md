# FORGE-31 — Trigger terminal summary on `completed` edge for disappeared tasks

## Root cause (confirmed from the code)

`Orchestrator._maybe_cancel_cleanup` (`src/saga/orchestrator/loop.py:475-489`) is the only handler invoked when a task drops off the active list. It bails on the very first check (`live[1] != "canceled"`), so any ticket Linear moves directly from `In Review` to `In Staging` (state_type `completed`) is silently skipped:

```python
if live is None or live[1] != "canceled":
    return
```

The only other call site that posts `aggregate_terminal` is `PrMonitor._finalize_merged` (`pr_monitor.py:168-188`), which is reachable only while the `pr_review` step is being dispatched — i.e. only while the card still sits in `In Review`. Linear's webhook-driven `In Review → In Staging` move happens before saga's 30s poll observes `pr.merged == True`, so in practice `_finalize_merged` rarely wins the race. The disappearance handler must therefore become the durable, idempotent backstop for the `completed` edge.

The existing `aggregate_terminal` (`steps/terminal/aggregate.py:447-452`) is already idempotent via `ts.aggregated_at`, and `_maybe_cancel_cleanup` already uses `last_processed_status` as a per-edge watermark — together these are the dual guards the fix relies on.

## Run / check commands (from `justfile` + `CLAUDE.md`)

- Tests: `just test` (`uv run pytest`)
- Single file: `uv run pytest tests/test_cancel_cleanup.py`
- Lint + types: `just lint` (`ruff check`, `ruff format --check`, `ty check`)
- Auto-fix: `just lint-fix`

The orchestrator itself can't be exercised locally without Linear/GitHub credentials, so the bug is reproduced and the fix is validated through `tests/test_cancel_cleanup.py`. The existing test at line 367 (`test_maybe_cancel_cleanup_skips_when_not_canceled`) documents the current broken behavior: it asserts that a `"completed"` live state results in *no* cleanup — which is exactly the bug. This test will be inverted as part of the fix.

## Files to change

1. `src/saga/orchestrator/loop.py` — refactor the disappearance handler into a dispatcher, add `_complete_cleanup`.
2. `tests/test_cancel_cleanup.py` — update the two tests that encode the broken behavior; add new tests for the `completed` path.

No other call sites reference `_maybe_cancel_cleanup` (verified via grep — `tick()` line 204 is the sole caller in production code; the test file is the only other reference).

## Change 1 — `src/saga/orchestrator/loop.py`

### 1a. Rename `_maybe_cancel_cleanup` → `_maybe_terminal_cleanup` and turn it into a dispatcher

Current shape (lines 475-489):

```python
async def _maybe_cancel_cleanup(self, task_id: str, old_task: LinearTask) -> None:
    live = await self.tracker.live_state(task_id)
    if live is None or live[1] != "canceled":
        return
    state_name = live[0]
    ts = await task_state_repo.get(task_id) or TaskState()
    if ts.last_processed_status == state_name:
        return  # already cleaned up this cancellation edge
    await self._cancel_cleanup(task_id, old_task, ts, state_name)
```

New shape:

```python
_TERMINAL_STATE_TYPES = frozenset({"completed", "canceled"})  # module-scope alongside _ABANDONED_STATE_TYPES

async def _maybe_terminal_cleanup(self, task_id: str, old_task: LinearTask) -> None:
    """Detect a terminal-edge for a disappeared task and run one-shot cleanup.

    list_tasks() filters by active column names, so both canceled AND completed tickets
    vanish without triggering _reconcile. The handler routes by state_type so that:
      - canceled → _cancel_cleanup (abort, close PRs, aggregate, post 🚫 comment)
      - completed → _complete_cleanup (abort, aggregate, NO PR close, NO 🚫 comment)
    Both branches share the `last_processed_status` watermark guard so a re-tick is a no-op,
    and `aggregate_terminal` itself remains exactly-once via `ts.aggregated_at`.
    """
    live = await self.tracker.live_state(task_id)
    if live is None or live[1] not in _TERMINAL_STATE_TYPES:
        return
    state_name, state_type = live
    ts = await task_state_repo.get(task_id) or TaskState()
    if ts.last_processed_status == state_name:
        return  # already cleaned up this terminal edge
    if state_type == "canceled":
        await self._cancel_cleanup(task_id, old_task, ts, state_name)
    else:  # "completed"
        await self._complete_cleanup(task_id, old_task, ts, state_name)
```

Also update the call site in `tick()` (line 204):

```python
await self._maybe_terminal_cleanup(task_id, prev_tasks[task_id])
```

And the `logger.exception` message at line 206 (`"cancel cleanup check failed"` → `"terminal cleanup check failed"`).

### 1b. Add `_complete_cleanup` next to `_cancel_cleanup`

Place it immediately after `_cancel_cleanup` (around line 556). Behavior — derived from the ticket's acceptance criteria and `_finalize_merged`'s exit state (so the two “PR merged” code paths converge on the same persisted state):

```python
async def _complete_cleanup(
    self, task_id: str, old_task: LinearTask, ts: TaskState, state_name: str
) -> None:
    """Ticket reached a `completed` status (typically Linear's GitHub integration moved the
    card to `In Staging` on merge before saga's poll observed `pr.merged`) — abort any
    in-flight agent, post the terminal aggregate, close the session, advance the watermark.

    Unlike `_cancel_cleanup`: PRs are merged (do NOT close them on GitHub) and no
    `🚫 Canceled` comment is posted (the aggregate IS the summary). `aggregate_terminal`
    is idempotent via `ts.aggregated_at`, so if `_finalize_merged` already posted, the
    aggregate call is a no-op for the comment side; the state writes below still advance
    `last_processed_status` so the next tick recognises this edge as processed.
    """
    # 1. Abort any in-flight agent.
    agent = self.state.running_agents.pop(task_id, None)
    if agent is not None:
        agent.abort()
    self.state.pending_tasks.discard(task_id)

    # 2. Post terminal aggregate (idempotent via ts.aggregated_at).
    try:
        await aggregate_terminal(
            task=old_task,
            cfg=self.cfg,
            tracker=self.tracker,
            notifier=self.notifier,
            terminal_status=state_name,
        )
    except Exception:
        logger.exception(f"failed to post terminal aggregate task={task_id}")

    # 3. Close the agent session.
    await self.session_mgr.close(task_id)

    # 4. Persist final state. Mirror `_finalize_merged`'s exit shape (prs cleared, pause
    #    cleared, watermark advanced) so the two completion paths converge — do NOT set
    #    pause=STOPPED here, the ticket is done not aborted, and `_finalize_merged` doesn't
    #    set it either. Touching step_records is also unnecessary (parity with
    #    `_finalize_merged`).
    await task_state_repo.update_state(
        task_id,
        session_id=None,
        branch_name=None,
        prs=[],
        pause=None,
        last_processed_status=state_name,
    )
    logger.info(f"complete cleanup done task={task_id} state={state_name!r}")
```

Notes on the choices above:
- **No `pause=Pause.STOPPED`** — that's a cancel-specific signal (“human aborted; saga should stay hands-off”). For a successfully completed ticket we want the natural `pause=None` so future logic (e.g. learning ticket selection) treats it normally.
- **No GitHub `close_pull` loop** — explicitly required by acceptance criterion #3.
- **No `🚫 Canceled` comment** — the terminal aggregate IS the ticket-level summary; a second comment would be noise.
- **`stage`, `consecutive_failures`, `step_records` left intact** — matches `_finalize_merged`'s exit state. The cancel path clears them only because cancellation is meant to be a hard reset; completion is a natural lifecycle end.

### 1c. Cleanup of `_cancel_cleanup` (no behaviour change)

Leave `_cancel_cleanup` and its sub-comments exactly as they are — the cancel path is unaffected. Only its docstring lead-in (`"""Ticket moved to canceled..."""`) may want a one-line tweak to clarify it’s called for the canceled edge only; not required for correctness.

## Change 2 — `tests/test_cancel_cleanup.py`

### 2a. Update tests that encode the old broken behavior

- **Line 367 — `test_maybe_cancel_cleanup_skips_when_not_canceled`**
  Currently asserts `_cancel_cleanup` is NOT called when `live_state` returns `("Done", "completed")`. Replace with a test that asserts `_complete_cleanup` IS called and `_cancel_cleanup` is NOT:

  ```python
  async def test_maybe_terminal_cleanup_routes_completed_to_complete_cleanup(...):
      backend = _FakeBackend(live=("In Staging", "completed"))
      orch = _make_orch(tmp_path, backend)
      orch._cancel_cleanup = AsyncMock()
      orch._complete_cleanup = AsyncMock()
      task_states["issue-1"] = TaskState(last_processed_status="In Review")
      await orch._maybe_terminal_cleanup("issue-1", _task())
      cast(Any, orch._cancel_cleanup).assert_not_called()
      cast(Any, orch._complete_cleanup).assert_awaited_once()
      args = cast(Any, orch._complete_cleanup).call_args
      assert args[0][0] == "issue-1"
      assert args[0][3] == "In Staging"
  ```

- **Line 463 — `test_tick_skips_completed_disappeared_task`**
  Currently asserts `_cancel_cleanup` is NOT spied on cleanup calls when a task disappears with `live_state == ("Done", "completed")`. Replace with the inverse: spy `_complete_cleanup` and assert it IS called. Rename to `test_tick_runs_complete_cleanup_for_completed_disappeared_task`.

- **Line 383 — `test_maybe_cancel_cleanup_skips_when_live_state_fails`**: still valid — rename the function reference from `_maybe_cancel_cleanup` to `_maybe_terminal_cleanup`. Keep the assertion: `live=None` → no cleanup.

- **Line 399 — `test_maybe_cancel_cleanup_is_idempotent`**: still valid for the cancel side — rename to `test_maybe_terminal_cleanup_idempotent_via_watermark_for_canceled` and call `_maybe_terminal_cleanup`.

- **Line 414 — `test_maybe_cancel_cleanup_fires_for_new_cancellation`**: still valid — rename to `test_maybe_terminal_cleanup_routes_canceled_to_cancel_cleanup` and call `_maybe_terminal_cleanup`.

- **Line 438 — `test_tick_detects_disappeared_canceled_task`**: no change beyond confirming the spy still hits `_cancel_cleanup` for `("Canceled", "canceled")`.

### 2b. Add new tests for `_complete_cleanup`

All using the existing `task_states` fixture + `_FakeBackend` / `_FakeWorkspace`. Tests to add (mirror the cancel suite, minus the cancel-specific assertions):

1. `test_complete_cleanup_posts_aggregate` — given `ts.prs = [PRState(...)]` (merged) and `ts.aggregated_at is None`, after `_complete_cleanup(... "In Staging")`:
   - A comment with `"Terminal Summary — In Staging"` is in `backend.comments`.
   - `ts.aggregated_at` is set (datetime).
   - State after: `prs == []`, `pause is None`, `last_processed_status == "In Staging"`.

2. `test_complete_cleanup_does_not_close_prs_on_github` — install `gh = _make_github()`, call `_complete_cleanup` with `ts.prs = [PRState(repo="org/repo", number=42)]`; assert `gh.close_pull` was **not** called. (Acceptance criterion #3.)

3. `test_complete_cleanup_posts_no_cancel_marker` — assert no comment body contains the `🚫 **Canceled**` lead-in. The only comment posted is the terminal aggregate.

4. `test_complete_cleanup_is_idempotent_when_finalize_merged_already_ran` — seed `ts.aggregated_at = datetime.now(UTC)` and `ts.last_processed_status = "In Staging"` (the exit state of `_finalize_merged`). Two layers:
   - Through `_maybe_terminal_cleanup`: watermark already matches `state_name="In Staging"` → no comments posted, no second aggregate, `_complete_cleanup` not invoked.
   - Direct call to `_complete_cleanup` (simulating a watermark drift scenario like `In Staging → Done`): no second `Terminal Summary` comment is posted (the `aggregated_at` guard fires inside `aggregate_terminal`); state writes still advance `last_processed_status` to the new state name.

5. `test_complete_cleanup_aborts_running_agent` — same pattern as the existing cancel test at line 241: a long-running asyncio task is registered in `running_agents`, `_complete_cleanup` runs, agent is cancelled and slot is freed.

6. `test_complete_cleanup_closes_session` — mock `orch.session_mgr.close` and assert it’s awaited with the task id.

7. `test_complete_cleanup_clears_prs_and_advances_watermark` — given `ts.prs = [...]`, `ts.session_id = "s-1"`, after the call: `prs == []`, `session_id is None`, `branch_name is None`, `last_processed_status == "In Staging"`, `pause is None`. Confirms parity with `_finalize_merged`'s exit state.

8. (Integration) `test_tick_runs_complete_cleanup_for_disappeared_completed_task` — task in `last_tasks`, `list_tasks()` returns `[]`, `live_state == ("In Staging", "completed")`, watermark `"In Review"`. Spy `_complete_cleanup`, assert it was called once with the task id. (Replaces / pairs with the cancel-side integration test at line 438.)

9. `test_maybe_terminal_cleanup_idempotent_via_watermark_for_completed` — watermark already `"In Staging"`, live state `("In Staging", "completed")` → neither `_cancel_cleanup` nor `_complete_cleanup` is invoked.

## Order of changes (dependencies first)

1. Add `_TERMINAL_STATE_TYPES` constant + `_complete_cleanup` method in `loop.py`.
2. Refactor `_maybe_cancel_cleanup` body and rename to `_maybe_terminal_cleanup`. Update the `tick()` call site and the surrounding exception log message.
3. Update existing tests in `tests/test_cancel_cleanup.py` to use the new name and (where applicable) the new completed-routing expectation.
4. Add the new `_complete_cleanup` tests + integration tick test.
5. `just lint && just test` until green.

## Edge cases & risks

- **`_finalize_merged` won the race in this tick**: the next tick won't even enter `_maybe_terminal_cleanup` (the task already disappeared from `last_tasks` once; nothing new dropped off). If it does (e.g. a later Linear move from `In Staging` → `Done`), `aggregated_at` is set and the aggregate is a no-op; `last_processed_status` advances to `"Done"`. ✅ Idempotent.
- **`aggregate_terminal` raises** (e.g. Linear comment API failure): wrapped in try/except (mirrors the cancel path). The watermark `last_processed_status` still advances, preventing an infinite retry storm. Trade-off: a transient failure means we don't get the summary. This matches the existing cancel-side behavior, and the comment is only “lost” in the rare error case; logs surface it.
- **Watermark drift after `_finalize_merged`** (`In Staging` → `Done`): `_complete_cleanup` runs but the aggregate guard fires; we just bump `last_processed_status` to `"Done"`. No double-post. ✅
- **Active task with `live_state` returning `started`**: untouched — `live[1] not in _TERMINAL_STATE_TYPES` returns immediately. ✅
- **Race between `_maybe_terminal_cleanup` and a still-running `pr_review` step**: the abort + `running_agents.pop` covers this. The aggregate is idempotent across both code paths.
- **`live_state` failure**: returns `None` → early return → fail-open (same as today). ✅
- **`Pause.STOPPED` semantics divergence**: cancel sets `STOPPED`, complete sets `None`. Verified this matches `_finalize_merged` (no `STOPPED` on a clean merge). Anything downstream that checks `pause` (e.g. routing, label projection) treats this correctly because the task no longer appears in `list_tasks()` anyway.

## Verification

1. **Static**: `just lint` passes (ruff + ty).
2. **Tests**: `uv run pytest tests/test_cancel_cleanup.py` shows the new + updated tests passing; the full `just test` is green.
3. **Behavior to observe in tests** (the “after”): the inverted `test_tick_*` tests that previously documented the bug now assert the corrected routing. Specifically:
   - Before: a `("Done", "completed")` disappearance did **not** trigger any cleanup. (Encoded in lines 367 + 463.)
   - After: a `("In Staging", "completed")` disappearance dispatches to `_complete_cleanup`, which posts the terminal summary exactly once (verified via `backend.comments` containing one `"Terminal Summary — In Staging"` body), does not call `gh.close_pull`, and persists `aggregated_at`, `prs=[]`, `last_processed_status="In Staging"`, `pause=None`.
4. The repo cannot be `saga run` end-to-end without Linear/GitHub credentials, so verification is test-based. This is consistent with the rest of the cancel-cleanup suite, which is the canonical test surface for this code path.

## Out of scope (per ticket)

- Layer-3 daily aggregation (`learning/ticket.py`) — untouched.
- `pr_review` poll cadence / Linear webhook integration — untouched.
- `aggregate_terminal` internals — only the new call site is added.
