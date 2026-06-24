# FORGE-12 — Make triage always produce an assessment

## Bug recap

In FORGE-2 the triage agent's turn completed normally (the SDK returned a successful `ResultMessage`), but the agent **never invoked the `mcp__saga__record_triage` MCP tool**. `TriageStep.post_step` then read `ts.triage`, found `None`, and called `needs_human` with `reason="Triage produced no assessment — the agent did not call record_triage."` — the comment quoted in the ticket.

Code path confirmed:
- `src/saga/orchestrator/steps/triage/__init__.py:129–143` (no-result branch in `post_step`)
- `src/saga/orchestrator/steps/generic.py:54` (`needs_human` posts `🙋 **Needs human** — {reason}`)

The triage prompt already tells the agent `**Do not end your turn without calling record_triage.**` (`src/saga/orchestrator/steps/triage/triage.md:183`), but prompt-only enforcement is not reliable — Step 0 already asks the agent to call `update_ticket_description`, so the agent has finished one tool call, paraphrased its reasoning, and stopped without making the final mandatory tool call. The same hole exists in the two sibling steps: `product_definition` and `technical_plan` have identical "no result → needs-human" branches (`product_definition/__init__.py:119–135`, `technical_plan/__init__.py:127–143`).

## Run / verify commands

- Tests: `uv run pytest tests/test_triage_step.py` (and `tests/test_product_definition_step.py`, `tests/test_technical_plan_step.py` if they exist) — all currently pass.
- Full suite: `uv run pytest`
- Lint/types: `uv run ruff check && uv run ruff format --check && uv run ty check`
- Justfile equivalents: `just test`, `just lint`.

The orchestrator (`saga run`) requires real Linear/Slack/Anthropic credentials and a live agent that "forgets" to call its tool, so the LLM-skip behavior is not reproducible inside this worktree — we anchor verification on unit tests that simulate the missing-tool outcome.

## Fix strategy — in-session nudge loop

Three layers of defense, in order:

1. **Layer 1 — in-turn nudge (new):** After the first agent turn completes with `event="completed"`, if the expected structured result is still missing from `TaskState`, send a short follow-up message to the **same live session** instructing the agent to call the tool now. Cap at **2 nudges per `work()` call**. The vast majority of misses resolve on nudge #1.
2. **Layer 2 — loop_back via on_failure (existing, now reached by Layer 1):** If both nudges fail, `work()` returns `WorkStatus.FAILED` with a clear summary. The runner's `on_failure` increments `consecutive_failures`, appends a `StepRecord(failure_class=AGENT_ERROR, summary=...)`, and re-enters `work()` on the next tick with the prior-attempt context injected by `format_prior_attempts`.
3. **Layer 3 — `max_attempts()` escalation (existing, unchanged):** If `consecutive_failures` reaches `step.max_attempts()` (default 3), `mark_locally_failed` is called and the ticket falls back to the human gate — same final behavior as today, but only after 3 ticks × (1 main turn + 2 nudges) = up to 9 turn-equivalents.

The fix lives entirely in the shared `GenericAgentStep` so all three "record_*" steps (triage, product_definition, technical_plan) inherit it.

## Files & changes

### 1. `src/saga/orchestrator/steps/step.py` — extend `GenericAgentStep`

Add two declarative class attributes and a helper, then wrap the existing `session.send(prompt)` happy path with the nudge loop.

- New class attribute `result_attr: str | None = None`.
  - `None` means "this step has no MCP record contract" — nudge loop is skipped (back-compat).
  - Subclasses (see files 2–4) set it to `"triage"` / `"product_definition"` / `"technical_plan"`. These are the existing `TaskState` typed accessor names.
- New class attribute `record_tool: str | None = None` (defaults to `f"record_{self.name}"` via a tiny helper) — used to build the nudge text. We can keep it simple by deriving it inside the helper, no attribute needed.
- New class constant or method `max_record_nudges() -> int` returning `2`. Cheap and overrideable per step/test.

Modify `GenericAgentStep.work()` (current lines 257–315):

```python
# After the "completed" branch:
await task_state_repo.update_state(task.id, session_id=outcome.session_id)

# In-session nudge loop: if the agent ended without calling its mandatory record_*
# MCP tool, send a short follow-up turn asking it to do so. Skip when reply_text
# is set (the human's reply is the authoritative input — never override it).
if self.result_attr is not None and ctx.reply_text is None:
    nudges = 0
    while nudges < self.max_record_nudges():
        ts = await task_state_repo.get(task.id) or TaskState()
        if getattr(ts, self.result_attr) is not None:
            break
        nudges += 1
        nudge_prompt = (
            f"You ended your turn without calling the `record_{self.name}` MCP tool, "
            f"which is required to complete the {self.name} step. "
            f"Call `record_{self.name}` now with your complete assessment — "
            f"do not output any other text first."
        )
        logger.warning(
            f"{self.name} nudge {nudges}/{self.max_record_nudges()}: "
            f"agent did not call record_{self.name} task={task.id}"
        )
        nudge_outcome = await session.send(nudge_prompt)
        if nudge_outcome.session_id is not None:
            await task_state_repo.update_state(task.id, session_id=nudge_outcome.session_id)
        if nudge_outcome.event == "failed":
            await ctx.deps.session_mgr.close(task.id)
            return StepOutcome(
                status=WorkStatus.FAILED,
                summary=f"{self.agent_noun} failed during record-tool nudge.",
                metrics=nudge_outcome.metrics or outcome.metrics,
            )
        # last metrics wins so post_step sees the latest token counts.
        outcome = nudge_outcome

    ts = await task_state_repo.get(task.id) or TaskState()
    if getattr(ts, self.result_attr) is None:
        logger.warning(
            f"{self.name} agent did not call record_{self.name} after "
            f"{self.max_record_nudges()} nudges task={task.id}"
        )
        return StepOutcome(
            status=WorkStatus.FAILED,
            summary=(
                f"Agent did not call `record_{self.name}` after "
                f"{self.max_record_nudges()} nudges."
            ),
            session_id=outcome.session_id,
            metrics=outcome.metrics,
        )

return StepOutcome(
    status=WorkStatus.DONE, session_id=outcome.session_id, metrics=outcome.metrics
)
```

Notes:
- We re-read `TaskState` after each nudge because `record_*` writes via `append_step_record` (see `mcp_tools.py:79`), so the in-memory `ctx.ts` snapshot is stale.
- We persist `session_id` after each nudge so a process restart between nudges doesn't lose the resume token.
- We funnel the missing-result outcome through `WorkStatus.FAILED` (not a new status), so the runner's existing `on_failure → loop_back → max_attempts` chain handles retry, attempt counting, and final escalation — no new orchestration code needed.

### 2. `src/saga/orchestrator/steps/triage/__init__.py`

In the `TriageStep` class body, add:

```python
result_attr = "triage"
```

The existing `post_step` no-result branch (lines 129–143) **stays** as a defensive safety net — with the new work-level FAILED path it should be unreachable, but keeping it preserves the regression-guard test `test_post_step_no_triage_result_calls_needs_human` and protects against any future regression in the nudge logic.

### 3. `src/saga/orchestrator/steps/product_definition/__init__.py`

Add `result_attr = "product_definition"` to `ProductDefinitionStep`. Leave the existing `post_step` safety branch in place for the same reason.

### 4. `src/saga/orchestrator/steps/technical_plan/__init__.py`

Add `result_attr = "technical_plan"` to `TechnicalPlanStep`. Same rationale.

### 5. `src/saga/orchestrator/steps/triage/triage.md`

Tighten the closing line of the prompt so the agent has unambiguous wording — defense in depth (the nudge does the real work, but a clearer prompt reduces the rate at which we need to nudge):

Replace the current "Do not end your turn without calling `record_triage`." with a more forceful, single-line directive that is the **last** thing the prompt says, e.g.:

```
**MANDATORY FINAL ACTION:** Your turn is not complete until you have called the `record_triage` MCP tool. Do not end your response with reasoning alone — the very last action of your turn must be a `record_triage` tool call. Use the exact lowercase enum values shown above.
```

(Apply the same minor wording tightening to `product_definition.md` and `technical_plan.md` for consistency.) Keep these edits small and surgical — the structural fix lives in code.

### 6. Tests — `tests/test_triage_step.py`

Add three new tests modelled on the existing `test_work_happy_path_returns_done`. They drive `session.send` with `side_effect` lists so the first call returns "completed" without populating `ts.triage`, then the nudge calls do (or don't) populate it.

- `test_work_nudges_when_record_triage_missing_then_succeeds`
  - `session.send.side_effect = [done_no_record, done_with_record]` where the second call's side_effect also writes a `TriageResult` into `task_states["issue-1"]` via the standard `_state` helper.
  - Assert: `session.send` called exactly twice; second call's prompt contains `record_triage`; final outcome status is `WorkStatus.DONE`.
- `test_work_nudges_exhausted_returns_failed`
  - `session.send` always returns "completed" without recording. `task_states["issue-1"]` stays empty.
  - Assert: `session.send` called 3 times (1 main + 2 nudges); outcome status is `WorkStatus.FAILED`; outcome summary mentions `record_triage`.
- `test_work_reply_text_skips_record_nudge`
  - `reply_text` is set; ts has no triage result. Assert: `session.send` called exactly once (the reply); no nudge.
- `test_work_failed_during_nudge_returns_failed`
  - First send is "completed" no-record; second send (nudge) returns `event="failed"`. Assert: `session_mgr.close` awaited; outcome status FAILED.

Add the corresponding two/three tests to `tests/test_product_definition_step.py` and `tests/test_technical_plan_step.py` (mirror the shape — keep them lean by parametrizing on the step where possible, or copy-paste-adapt to match the existing style of each file).

### 7. Sanity-check: keep `post_step` safety net behavior verified

Leave the existing tests `test_post_step_no_triage_result_calls_needs_human` and `test_post_step_no_triage_result_label_not_applied_sets_failed` unchanged. They now exercise the *defense-in-depth* branch (only reachable if the work-level nudge logic is bypassed in a test). This is the regression guard against future drift.

## Order of changes

1. Add the nudge loop to `GenericAgentStep.work()` in `step.py`. Run `uv run pytest tests/test_triage_step.py` — existing tests must continue to pass (back-compat: `result_attr` defaults to `None` so behavior is unchanged until step 2).
2. Set `result_attr = "triage"` on `TriageStep`. Add the four new tests in `tests/test_triage_step.py`. Run the triage test file.
3. Set `result_attr` on `ProductDefinitionStep` and `TechnicalPlanStep`. Add nudge tests in the corresponding test files (if they exist; if not, add them sibling to `test_triage_step.py`).
4. Tighten the final-action wording in `triage.md` / `product_definition.md` / `technical_plan.md`.
5. `uv run pytest` (full suite) + `uv run ruff check && uv run ruff format --check && uv run ty check`.

## Edge cases & risks

- **Stale `ctx.ts`.** The fresh `task_state_repo.get` after each nudge is mandatory — `ctx.ts` is the snapshot at dispatch time and never updates in place. The current `work()` already re-reads via `task_state_repo.update_state` calls; we extend that pattern.
- **Reply-driven dispatches.** When `ctx.reply_text` is set, the human's text is the authoritative input; we must not append a nudge. The plan gates the loop on `ctx.reply_text is None`.
- **Session failure mid-nudge.** The nudge `session.send` can itself return `event="failed"` (SDK crash, network). In that case we close the session and return `WorkStatus.FAILED` — mirrors the original first-turn failure handling.
- **Metrics double-counting.** Each `session.send` returns its own `StepMetrics`. We carry the latest non-None metrics so the post-step Linear/Slack outcome reflects the actual turn cost; we do not sum across nudges (keeps the existing per-step metric semantics).
- **Cost upper bound.** Worst case per ticket: 3 ticks × (1 main turn + 2 nudges) = 9 LLM round-trips before `needs_human`. Nudge prompts are 2–3 sentences so the marginal cost is small relative to the main triage turn.
- **The `update_ticket_description` half-completion.** If the agent updated the description but skipped `record_triage`, the description is already persisted. That's fine — the nudge will then drive `record_triage` and the assessment is durable. No rollback needed.
- **Mandatory-approve gate steps.** `technical_plan` can be put behind a human approval gate (`mandatory_approve`). The nudge loop runs *before* gating, so the gate still sees a populated `ts.technical_plan` — unchanged behavior on the gated path.
- **Verifier retry interaction.** The verifier loop (`record_verifier_fail` → `loop_back`) is unaffected: it only runs in `post_step` after `work()` has returned DONE with a populated result. The nudge loop strictly precedes the verifier.

## How to verify

- Before: `uv run pytest tests/test_triage_step.py` — 33 tests pass on master, including the existing "no result → needs-human" defense.
- After: `uv run pytest tests/test_triage_step.py` — original 33 + new 4 nudge tests pass. Equivalent for product_definition and technical_plan.
- Full suite: `uv run pytest`.
- Lint: `uv run ruff check && uv run ruff format --check && uv run ty check`.
- Observable behavior change: in the saga orchestrator log, a previously-failing ticket would have shown a single `triage produced no assessment` warning before the needs_human pause; post-fix, the log will show `triage nudge 1/2: agent did not call record_triage` (and usually nothing else) followed by normal `triage ready; advancing`. The `🙋 Needs human — Triage produced no assessment` Linear comment should disappear from the system entirely in routine operation.