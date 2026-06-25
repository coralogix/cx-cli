# FORGE-23 — Triage nudge loop must surface validation errors

## Root cause

`GenericAgentStep.work()` (`src/saga/orchestrator/steps/step.py:325-376`, added in FORGE-12) sends a **single static nudge prompt** when the step's record_* MCP tool was not persisted by the end of a turn:

```python
nudge_prompt = (
    f"You ended your turn without calling the `record_{self.name}` MCP tool, "
    f"which is required to complete the {self.name} step. "
    f"Call `record_{self.name}` now with your complete assessment — "
    f"do not output any other text first."
)
```

This wording is correct for the original failure mode (agent simply forgot the tool), but it is **wrong and unhelpful** when the agent *did* call the tool and the tool returned `is_error: True`. `record_triage` returns `is_error: True` whenever:

- An enum value is wrong (`risk`, `complexity`, `ticket_type`, `dor`) — `src/saga/services/claude/mcp_tools.py:81-87`, via `TriageResult(**args)` pydantic `ValidationError`.
- The `repos` list contains a key outside `cfg.repos` — `mcp_tools.py:65-79`.

In that scenario the agent saw a `ToolResultBlock` with `is_error: True` and an error message, decided the call was unrecoverable, and ended its turn. The nudge then tells it "you didn't call the tool" — which contradicts what the agent just observed — and gives **zero detail about why the call failed**. The agent's likely responses are: repeat the same bad call, give up, or guess randomly. All three exhaust the nudge budget with `ts.triage` still `None`, ending in `WorkStatus.FAILED` → `needs_human`. This is exactly what FORGE-23 reports.

The `outcome.chat_history` returned from `session.send()` already carries the full transcript of the just-completed turn, including `ToolUseBlock`s (with name `mcp__saga__record_<step>`) and the matching `ToolResultBlock`s (with `is_error` and `content`). We have everything needed to build a context-aware nudge — we just don't read it.

## Scope of fix

The fix lives entirely inside `GenericAgentStep.work()`'s nudge loop, so it covers all three steps that use `result_attr` (`triage`, `product_definition`, `technical_plan`) — exactly as the ticket's "Out of scope" note expects.

We do **not** touch:
- `TriageResult` / other pydantic models or the MCP tool interfaces.
- Triage prompt structure (`triage.md`) beyond, optionally, the nudge text.
- Any other step's `post_step` / `verify`.

## Changes

### 1. `src/saga/orchestrator/steps/step.py`

Add a small helper inside the step module (module-level private function) that inspects a turn's `chat_history` for the most-recent `record_<self.name>` MCP call:

```python
from claude_agent_sdk import AssistantMessage
from claude_agent_sdk.types import (
    Message, ToolResultBlock, ToolUseBlock, UserMessage,
)

def _last_record_tool_error(
    history: list[Message], tool_name: str
) -> str | None:
    """Return the error text of the most recent failed call to ``tool_name`` in
    ``history``, or ``None`` if no failed call is present.

    ``tool_name`` is the bare MCP tool name (e.g. ``"record_triage"``). The full
    SDK name is ``mcp__saga__<tool_name>``.
    """
    full_name = f"mcp__saga__{tool_name}"
    # Walk forward, remembering each record_* tool_use_id as we see it; pair
    # with the next ToolResultBlock whose tool_use_id matches. We want the LAST
    # failed one (most recent), so update on every match.
    pending: dict[str, ToolUseBlock] = {}
    last_error: str | None = None
    for msg in history:
        if isinstance(msg, AssistantMessage):
            for block in msg.content:
                if isinstance(block, ToolUseBlock) and block.name == full_name:
                    pending[block.id] = block
        elif isinstance(msg, UserMessage) and isinstance(msg.content, list):
            for block in msg.content:
                if (
                    isinstance(block, ToolResultBlock)
                    and block.tool_use_id in pending
                    and block.is_error
                ):
                    last_error = _render_tool_error(block.content)
                    pending.pop(block.tool_use_id, None)
    return last_error


def _render_tool_error(content: str | list[dict[str, Any]] | None) -> str:
    """Flatten a ToolResultBlock.content payload to a single error string."""
    if content is None:
        return "(no error detail)"
    if isinstance(content, str):
        return content
    parts: list[str] = []
    for item in content:
        if isinstance(item, dict):
            text = item.get("text") or item.get("content")
            if text:
                parts.append(str(text))
    return "\n".join(parts) or "(no error detail)"
```

(Imports added at the top of the module. `Any` is already imported indirectly through other modules; add `from typing import Any` if needed.)

Then in `GenericAgentStep.work()`, replace the static nudge prompt block (`step.py:335-344`) with:

```python
nudges += 1
prior_error = _last_record_tool_error(outcome.chat_history, f"record_{self.name}")
if prior_error is not None:
    nudge_prompt = (
        f"Your previous call to the `record_{self.name}` MCP tool returned an "
        f"error and was NOT persisted:\n\n{prior_error}\n\n"
        f"Read the error carefully, fix ONLY the field(s) it names, and call "
        f"`record_{self.name}` again with the corrected payload. "
        f"Do not output any other text first."
    )
else:
    nudge_prompt = (
        f"You ended your turn without calling the `record_{self.name}` MCP tool, "
        f"which is required to complete the {self.name} step. "
        f"Call `record_{self.name}` now with your complete assessment — "
        f"do not output any other text first."
    )
logger.warning(
    f"{self.name} nudge {nudges}/{self.max_record_nudges()}: "
    f"agent did not persist record_{self.name} task={ctx.task.id} "
    f"prior_error={'yes' if prior_error else 'no'}"
)
nudge_outcome = await session.send(nudge_prompt)
```

Two other small but necessary tweaks in the same loop:

- After a failed nudge call (`nudge_outcome` returns), keep using `nudge_outcome.chat_history` as the source for `_last_record_tool_error` on the *next* iteration. This is already the effect of `outcome = nudge_outcome` at the end of the loop body — confirm and keep it.
- Leave the `reply_text is None` guard and `max_record_nudges()` budget unchanged. The nudge loop's structure stays identical; only the prompt is now informed by history.

No imports of `_render_content` from `session_transcript.py` — we want a standalone helper without coupling to the transcript module.

### 2. `tests/test_triage_step.py`

Add four regression tests, all under the existing "in-session nudge loop (FORGE-12)" section.

A small fixture helper (top of the file or alongside other helpers) to build a fake `chat_history` containing a record_triage tool call and an error result:

```python
def _history_with_failed_record(
    tool_name: str = "record_triage",
    error_text: str = "Validation error: 1 validation error for TriageResult\\nrisk\\n  Input should be 'trivial', 'low', 'medium' or 'high' [type=enum, input_value='extreme', input_type=str]",
) -> list:
    from claude_agent_sdk import AssistantMessage
    from claude_agent_sdk.types import (
        TextBlock, ToolResultBlock, ToolUseBlock, UserMessage,
    )
    full = f"mcp__saga__{tool_name}"
    return [
        AssistantMessage(
            content=[
                ToolUseBlock(id="use-1", name=full, input={"risk": "extreme"}),
                TextBlock(text="Calling record_triage."),
            ],
            model="claude",
        ),
        UserMessage(
            content=[
                ToolResultBlock(
                    tool_use_id="use-1",
                    content=[{"type": "text", "text": error_text}],
                    is_error=True,
                ),
            ],
        ),
    ]
```

(The exact `AssistantMessage` / `UserMessage` constructor signatures must match the SDK — confirm against `claude_agent_sdk.types`; the test currently does not instantiate these so we'll mirror the pattern used in `tests/test_session_transcript.py` where they are already constructed for tests.)

Tests to add:

#### `test_work_nudge_surfaces_validation_error`
Main turn returns `event="completed"` but with `chat_history` containing a failed `record_triage` call (validation error text). State has no triage result. Assert:
- Nudge is sent (`session.send.call_count == 2`).
- The nudge prompt text contains the original error text (e.g. "Validation error", "extreme").
- The nudge prompt **does not** say "you ended your turn without calling".

#### `test_work_nudge_surfaces_invalid_repos_error`
Same shape, but error text is `"Invalid repos: ['repo-x']. Allowed repos: ['my-service']."`. Assert nudge prompt contains "Invalid repos" and "my-service".

#### `test_work_nudge_generic_when_no_record_call_in_history`
Main turn's `chat_history` contains only an `AssistantMessage` with a `TextBlock` — no record_triage tool call at all. Assert nudge prompt still contains "ended your turn without calling" (the legacy text) and does NOT contain "returned an error".

#### `test_work_second_nudge_surfaces_first_nudge_error`
Main turn: no record call (legacy nudge fires).
First nudge turn: agent calls `record_triage` but gets a different validation error (e.g. wrong `ticket_type`).
Second nudge: assert the prompt now surfaces the first nudge's error text and that the call count is 3.

We can implement this by giving `session.send` an `AsyncMock` with `side_effect` that returns three distinct `SessionTurnOutcome`s, each with its own `chat_history`.

#### Update the existing `test_work_nudge_fires_then_agent_records_returns_done`
No behaviour change needed if the test asserts `"record_triage" in nudge_prompt` only — that substring is still present in the new generic and error-aware branches. Re-run after the edit; if assertions are stricter (they're not, currently), relax them.

### 3. Prompt tweak (defense-in-depth, optional but cheap)

In `src/saga/orchestrator/steps/triage/triage.md`, the "MANDATORY FINAL ACTION" paragraph (lines 186-187) already exists. Append a single sentence:

> If `record_triage` returns an `is_error` response, **read the error text, fix only the field it names, and call `record_triage` again** in the same turn. Do not end your turn until `record_triage` returns successfully.

Mirror the same edit in `technical_plan.md` and `product_definition.md` for consistency. Two lines per file. Keep it minimal — the real fix is in code, this is just a hint.

(Out-of-scope note in the ticket says "no refactoring beyond the nudge/validation fix" — adding one sentence to the existing mandatory-action paragraph qualifies as a focused nudge clarification, not a refactor.)

## Order of changes

1. Edit `src/saga/orchestrator/steps/step.py`:
   - Add the two helpers `_last_record_tool_error` and `_render_tool_error` at module scope.
   - Update the nudge loop's prompt construction to branch on `_last_record_tool_error`.
   - Add the required imports (`AssistantMessage`, `UserMessage`, `ToolUseBlock`, `ToolResultBlock`, `Any`).
2. Add the four new tests in `tests/test_triage_step.py` (plus the `_history_with_failed_record` helper).
3. Add the one-sentence prompt addendum to `triage.md`, `technical_plan.md`, `product_definition.md`.
4. Run `just lint && just test`. Fix any ruff/ty findings (likely just import order).

## Edge cases & risks

- **Multiple record_* calls in one turn.** The agent may retry inside the same turn. We want the *last* failed one — handled: we overwrite `last_error` on every match and don't carry forward a result that later succeeded (the loop only updates on `is_error=True`).
- **Successful call followed by `ts.triage` write race.** If the agent's call succeeded but `ts.triage` is still `None` because of a stale read, the helper returns `None` and we fall back to the generic nudge — same behaviour as today. We don't make this any worse.
- **`outcome.chat_history` is empty** (an SDK quirk or a `event="failed"` turn). The helper returns `None`; we fall back to the generic nudge. No crash because we iterate an empty list.
- **`UserMessage.content` is a `str`, not a list of blocks** (the SDK allows both forms). The helper's `isinstance(msg.content, list)` guard skips that case cleanly. Tool results are always carried as lists in practice, so we don't lose anything.
- **Error text containing markdown backticks.** Pydantic's `ValidationError.__str__()` includes the type tag in square brackets; safe to embed verbatim in the nudge prompt.
- **Token cost.** A validation error string is small (~200 chars); the prompt grows by < 1KB. Negligible vs. main turn.
- **Backwards compatibility.** Existing `test_work_nudge_fires_then_agent_records_returns_done` and `test_work_nudges_exhausted_returns_failed` keep passing because the new generic branch produces a prompt that still contains the `record_<step>` substring those tests assert on. Confirmed by reading their assertions (lines 1083-1084 and 1117 of `tests/test_triage_step.py`).
- **`reply_text` path.** Unchanged — nudge loop is already bypassed when `reply_text is not None` (`step.py:328`).
- **Shared helper used by all three steps.** Because the fix is in `GenericAgentStep`, `technical_plan` and `product_definition` automatically benefit. The ticket's "Out of scope" note explicitly blesses this.

## How to verify

### Run / check commands (from `saga/`)

| What | Command |
|---|---|
| Lint + types | `just lint` |
| Full test suite | `just test` |
| Targeted (this fix) | `uv run pytest tests/test_triage_step.py tests/test_technical_plan.py tests/test_product_definition.py tests/test_mcp_record_triage.py -q` |

Pre-fix baseline (already captured): `uv run pytest tests/test_triage_step.py -q` → 37 passed.

### Before-state observation

The bug cannot be reproduced end-to-end in this worktree (no live Linear/Slack/Claude SDK) — the description even calls out "no specific reproduction steps available". The closest deterministic reproduction is the unit-level scenario the new tests encode: a `chat_history` containing a failed `record_triage` call, the nudge prompt should mention the error. Run `uv run pytest tests/test_triage_step.py::test_work_nudge_surfaces_validation_error -q` after writing the test (before the code change) — the test should **fail** because the current nudge prompt is generic. This *is* the before-state.

### After-state observation

After the fix:
- The four new tests pass.
- All 37 existing triage tests still pass.
- `just lint && just test` is green.
- For the real system: when a triage agent passes `risk="extreme"`, the nudge now reads literally:
  > "Your previous call to the `record_triage` MCP tool returned an error and was NOT persisted: Validation error: 1 validation error for TriageResult / risk / Input should be 'trivial', 'low', 'medium' or 'high' …"
  > "Read the error carefully, fix ONLY the field(s) it names, and call `record_triage` again …"

That gives the agent enough to fix the call on the next nudge turn, satisfying success criterion #2 ("no `record_triage` validation failures go unhandled").

### Success-criteria mapping

| Ticket criterion | How the fix satisfies it |
|---|---|
| 1. Regression test in `tests/test_triage_step.py` | Four new tests under the FORGE-12 nudge-loop section. |
| 2. Validation failures are not silently dropped | `_last_record_tool_error` reads `outcome.chat_history` and surfaces the error to the agent in the very next prompt; the nudge budget (2) then gives the agent two corrected attempts before failing out. |
| 3. Nudge loop still fires correctly | `test_work_nudge_fires_then_agent_records_returns_done` and `test_work_nudges_exhausted_returns_failed` remain green (their assertions are unchanged-compatible). |
