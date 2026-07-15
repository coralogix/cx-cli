
# FORGE-455 — Saga bug: prompt too long

## Summary of change

Bound the size of prompts Saga sends to Claude and instruct the agent to reply concisely, so oversized inputs like FORGE-359's 11 KB restructured description + accumulated plan/prior-attempts cannot silently balloon the composed prompt past the model's context window. Also add a diagnosable failure path (log + classify prompt-too-long errors) and a per-step config seam for escalating to a larger-context model *only if* the investigation step confirms window headroom is the actual binding constraint.

The change touches only prompt construction (`schemas/task.py`, `services/claude/prompts.py`), the session dispatch retry path (`orchestrator/steps/step.py`, `implementation/__init__.py`, `review/pr_monitor.py`), and adds one shared prompt fragment plus a small utility. No task-state schema change, no Linear or GitHub API change.

---

## Ground truth (before-state)

**Environment / run commands** (from `saga/CLAUDE.md` and `saga/justfile`):

- Install: `uv sync`
- Tests: `just test` (i.e. `uv run pytest`); single file: `uv run pytest tests/test_prompts.py`
- Lint + types: `just lint` (`ruff check`, `ruff format --check`, `ty check`)
- Auto-fix: `just lint-fix`

**Reproduction attempt.** The bug cannot be reproduced deterministically in this worktree — it fires only when a real Claude API turn rejects a real oversized prompt. What *is* observable:

- FORGE-359's restructured Linear description = ~11 KB / ~2.8K tokens (measured via `mcp__linear__get_issue FORGE-359`).
- FORGE-359 technical_plan step logged 61 turns, `cache_read_input_tokens ≈ 127K per turn` average (from the ticket's summary metrics) — i.e. per-turn effective context is ~127K, well under `claude-sonnet-5`'s 200K window but consistent with a long-running session that grows toward it as retries stack up.
- Two `🙋 Needs human — implementation failed after 3 attempts` comments (2026-07-10, 2026-07-15) are the *circumstantial* evidence — no logged `context_length_exceeded` / prompt-too-long line has been located in this worktree. The ticket owner explicitly flagged this: "which I assume is not but don't know."

The three unbounded-injection sites confirmed by direct read:

1. `src/saga/schemas/task.py:50-63` — `format_task_context_markdown` inlines the raw `ctx.description` verbatim, with no cap. Called from every step's prompt-context path (`step.py:349`, and by the pre-step / verifier / classifier via `format_task_context_markdown`).
2. `src/saga/services/claude/prompts.py:104-118` — `build_prompt` prepends the whole task context to every step's prompt body, with no cap.
3. `src/saga/services/claude/prompts.py:164-204` — `build_implementation_prompt` inlines the full `plan_text` verbatim.

Existing size guards protect *other* systems only: `task_state_store._MAX_BODY_CHARS = 60_000` (Linear body limit), `session_transcript._MAX_TOOL_RESULT_CHARS = 4000` (transcript file only), `sessions._CLAUDE_MD_SIZE_WARN_BYTES = 10 KB` (warning only, no action). `saga.util.truncate` exists but is not called from any prompt-construction site.

Failure handling for a Claude turn (`step.py:373-393`, mirrored in `implementation/__init__.py:319-343`, `review/pr_monitor.py:494-511`, `session.py:129-148`) treats every `ProcessError` as "likely stale session" and retries fresh once. It never inspects the stderr for a prompt-too-long signature, never shrinks the prompt, and never escalates the model.

The SDK does have a knob for this if we ever need it: `ClaudeAgentOptions.betas = ["context-1m-2025-08-07"]` enables a 1M-token window (Sonnet 4/4.5 only per `types.py:1687`), and `ClaudeAgentOptions.fallback_model` sets a fallback. Adding this is *conditional* on the investigation, per the ticket.

---

## Design decisions

- **Head-and-tail truncation, not head-only truncation.** For a Linear ticket description, the *goal / acceptance criteria* live near the top and *out-of-scope / related tickets* live near the bottom; both are load-bearing. `saga.util.truncate` currently only tail-truncates. Extend the util with `truncate_middle(text, max_chars, notice=...)` that keeps a head slice and a tail slice with a `… [truncated N chars] …` marker in between. Preserve `truncate` unchanged for existing callers.
- **Cap per-injection-site, not per-composed-prompt.** Composing the whole prompt then truncating loses locality — you can't tell which section blew up. Cap each injection site (`description`, `plan_text`, `format_prior_attempts`) with its own budget, and log the composed length once at build time.
- **Concrete caps** (rounded, easy to reason about, all comfortably within a 200K window even at 4 chars/token):
  - `_MAX_DESCRIPTION_CHARS = 20_000` — keeps FORGE-359 whole; still bounds pathological cases.
  - `_MAX_PLAN_TEXT_CHARS = 40_000` — the plan is the load-bearing input for `implementation`; needs headroom, but not unbounded. When exceeded, replace inline plan text with `See ./<repo>/plans/<identifier>.md in your worktree.` — the implementation step already reads that file directly (`implementation/__init__.py:_resolve_plan_text`), so the agent has a first-class alternate path.
  - `_MAX_PRIOR_ATTEMPTS_CHARS = 8_000` — accumulates fast on retries; head-truncate each individual attempt summary at ~500 chars and cap the composed block.
- **Concise-output instruction is a shared fragment.** Add `orchestrator/steps/_shared/response_brevity.md` (analogous to `feedback_loop.md`), inject via `{{shared:response_brevity}}` at the top of each `<step>.md`. Content: brief bullets on preferring short bullets over prose, not restating context, and (for `technical_plan`) not re-quoting large source blocks in the plan text itself (a re-quote of a 300-line file will blow past `_MAX_PLAN_TEXT_CHARS` on its own — the ticket-cited "have the agent respond with fewer words" fix). Because the fragment is expanded at prompt-load time, this also propagates automatically to any future step that includes it.
- **Detect but don't act blindly on prompt-too-long.** In `ClaudeAgentSession.send`, when a `ProcessError` fires, additionally inspect `stderr` for the well-known Claude/Anthropic error substrings (`prompt is too long`, `context_length_exceeded`, `input length and `max_tokens``). Raise a dedicated `PromptTooLongError` subclass of `ProcessError` so upstream callers can distinguish it from "stale session".
- **Corrective action on `PromptTooLongError`.** In the three retry sites (`step.py`, `implementation/__init__.py`, `review/pr_monitor.py`), when the error is a `PromptTooLongError`:
  1. Drop `session_id` (reset to fresh) — resumed history is the biggest amplifier.
  2. Drop `format_prior_attempts` from the retry prompt (it's the fastest-growing accumulator).
  3. If a `long_context_model` is configured for this step, use it on the retry.
  4. Log a `logger.error(f"prompt-too-long detected step={self.name} task={task.id}; retrying with shrunk prompt + fresh session")` line.
   
   This is a single retry (matches existing `ProcessError` retry cadence). If the retry also fails, `mark_locally_failed` still fires — but the failure message names "prompt too long" explicitly rather than an opaque exit code.
- **Config seam for the larger-context model** (conditional on the investigation, but the seam is cheap and lets the change land as one PR):
  - Add optional `long_context_model: str | None = None` to `PhaseCfg` (inherited by `TriageCfg`, `ProductDefinitionCfg`, `TechnicalPlanCfg`, `ImplementationPhaseCfg`).
  - Add optional `long_context_betas: list[str] = []` to `AgentCfg`. Documented value: `["context-1m-2025-08-07"]` (SDK-supported per `claude_agent_sdk/types.py:1687`).
  - Thread through to `AgentRequest`: add `long_context_model: str | None = None` and `long_context_betas: list[str] = []`. `sessions.get_or_open` passes them into `ClaudeAgentOptions` only on the fallback retry path — the default (`long_context_model=None`) is a strict no-op.
- **Investigation deliverable.** The plan itself documents (in this text and in code comments on the new detection path) that we don't have a confirmed context-window-exceeded log line yet. When the detection landed in `ClaudeAgentSession.send` fires for the first real ticket, the log line + captured stderr become the confirmation the ticket owner asked for. The `long_context_model` config knob is present but unused until an operator opts in — no default model change, per the "Switching the model is not assumed to be needed" out-of-scope note.
- **What we deliberately do NOT change:**
  - Don't touch `_MAX_BODY_CHARS` (60_000) — that guards Linear's server-side limit and is unrelated.
  - Don't touch `_MAX_TOOL_RESULT_CHARS` (4000) — that guards transcript-file readability, not the LLM prompt.
  - Don't add a chat-history compaction pass for resumed sessions — Claude Code's own autocompact handles that (`ContextUsageResponse.isAutoCompactEnabled`); duplicating it here would fight the CLI. We only *avoid* stacking large single-turn injections on top of an already-large resumed history.
  - Don't change the default model (`claude-sonnet-5` stays). The long-context path is opt-in.

---

## Order of changes (dependencies first)

### 1. Utilities (bottom-of-graph, no dependencies)

**File:** `src/saga/util.py`

- Add `truncate_middle(value: str, max_chars: int, *, head_ratio: float = 0.6, notice: str = "…[truncated {n} chars]…") -> str`.
  - When `len(value) <= max_chars`, return unchanged.
  - Otherwise split: keep `head_chars = int((max_chars - len(notice_rendered)) * head_ratio)` from the front, keep `tail_chars = (max_chars - len(notice_rendered)) - head_chars` from the back, insert the rendered notice between them (with actual `n = len(value) - head_chars - tail_chars`).
  - Never returns something longer than `max_chars` (guard by falling back to `truncate(value, max_chars)` if `max_chars` is too small to fit head + notice + tail).
  - Add tests in `tests/test_util.py` (create if absent — checked, absent today): short/short-equal, oversize with head+tail preserved, `max_chars` too small falls back cleanly, unicode-safe (no split inside a multi-byte codepoint — use string slicing, not bytes).

### 2. Description + plan-text truncation constants and helpers

**File:** `src/saga/services/claude/prompts.py`

- At the module top, add:
  ```
  _MAX_DESCRIPTION_CHARS = 20_000
  _MAX_PLAN_TEXT_CHARS = 40_000
  _MAX_PRIOR_ATTEMPTS_CHARS = 8_000
  _PLAN_PLACEHOLDER = "\n\n> _(plan omitted — read `./{repo}/plans/{identifier}.md` in your worktree for the full text)_\n"
  ```
- Update `format_prior_attempts` (`prompts.py:80-101`): before returning, if the composed string exceeds `_MAX_PRIOR_ATTEMPTS_CHARS`, apply `truncate_middle` with a notice referencing how many older attempts were elided; also apply a `truncate` per individual `detail` at ~500 chars so a single verbose failure summary can't dominate.
- Update `build_implementation_prompt` (`prompts.py:164-204`):
  - Accept a new optional `plan_placeholder_target: str | None = None` (the worktree path Saga wants to point the agent at). Default falls back to the generic "your worktree's plans/ directory".
  - If `len(plan_text) > _MAX_PLAN_TEXT_CHARS`, log `logger.warning(f"implementation prompt: plan_text {len(plan_text)} > cap; substituting placeholder")` and use `truncate_middle(plan_text, _MAX_PLAN_TEXT_CHARS, ...)` followed by the placeholder pointing at the on-disk plan file. Do not silently drop the plan — the truncated head+tail carries the plan's intent; the placeholder tells the agent where to read the rest.
- Add a private helper `_bounded_task_context(ctx: LinearTaskContext) -> LinearTaskContext` OR add the cap inside `format_task_context_markdown` directly (see step 3) — decision below.

### 3. Description truncation at the injection site

**File:** `src/saga/schemas/task.py`

- Import `truncate_middle` from `saga.util` and the cap `_MAX_DESCRIPTION_CHARS` (either duplicate the constant here or expose it from `prompts.py`; prefer duplication since `task.py` is a schema module and shouldn't import from `services/*`).
- Update `format_task_context_markdown` to truncate `ctx.description` via `truncate_middle(ctx.description, _MAX_DESCRIPTION_CHARS, notice=...)` when non-null, before appending it.
- Log the truncation once, at DEBUG (or WARNING when it actually fires) via a module logger `saga.schemas.task` — we don't have a logger there today; add one.
- Rationale for putting the cap here rather than in `prompts.py`: every caller of `format_task_context_markdown` (5 sites — step main turn, pre-step, verifier for triage/prod-def/tech-plan/implementation) benefits, without each caller having to remember to opt in. This is the single choke point.

### 4. Concise-response shared fragment

**Files:**
- **New:** `src/saga/orchestrator/steps/_shared/response_brevity.md` — content along the lines of:
  ```
  ## Response style

  Be terse. Prefer short bullets over prose. Do not restate context that was
  already given to you in this prompt. When quoting code, quote only the
  smallest span needed to make the point — do not paste large files verbatim
  into your response or into a plan. Long responses cost real tokens and, in
  large tasks, push the conversation past the context-window limit.
  ```
- Update `src/saga/orchestrator/steps/triage/triage.md` — insert `{{shared:response_brevity}}` after the file header (before "What to read").
- Update `src/saga/orchestrator/steps/product_definition/product_definition.md` — same.
- Update `src/saga/orchestrator/steps/technical_plan/technical_plan.md` — same, and additionally add one plan-specific line into the brevity fragment or after its inclusion: "Cite files as `file:line`; do not paste code blocks longer than ~20 lines into `plan_text`." This is the FORGE-359 lesson — the plan text is what implementation resumes on, and inlined code snippets dominate its size.
- Update `src/saga/orchestrator/steps/implementation/implementation.md` — same.
- Prompt-parsing tests (`tests/test_prompts.py`) already verify that no `{{shared:…}}` token survives expansion — the new fragment will be caught by the same assertions. Add one new test asserting the brevity fragment expands and contains the substring `Be terse.`.

### 5. Session-side detection of prompt-too-long

**File:** `src/saga/services/claude/session.py`

- Add module-level constant:
  ```
  _PROMPT_TOO_LONG_MARKERS: tuple[str, ...] = (
      "prompt is too long",
      "context_length_exceeded",
      "input length and `max_tokens`",
  )
  ```
- Add exception class `PromptTooLongError(ProcessError)` — carries same `exit_code`/`stderr` fields.
- Update the `except ProcessError as exc:` block in `ClaudeAgentSession.send` (`session.py:137-148`): after capturing `stderr`, do a case-insensitive substring check against `_PROMPT_TOO_LONG_MARKERS`; if any matches, raise `PromptTooLongError(...)` in place of the enriched `ProcessError`. Also detect from the drained `chat_history` when a `ResultMessage` with `is_error=True` carries the same marker (some API rejections come back as a failed `ResultMessage`, not a subprocess crash — check the ResultMessage `result`/`error` fields via `getattr(msg, "result", None)`).
- Rationale: this is the single choke point every dispatch site funnels through. Doing the detection here means all three retry sites see the same typed exception.

### 6. Corrective retry on prompt-too-long

**Files:**
- `src/saga/orchestrator/steps/step.py` — `GenericAgentStep.work` (`step.py:373-393`).
- `src/saga/orchestrator/steps/implementation/__init__.py` — `ImplementationStep.work` (`__init__.py:319-343`).
- `src/saga/orchestrator/steps/review/pr_monitor.py` — the fix-turn dispatch (`pr_monitor.py:494-511`).

In each site, expand the existing `except ProcessError` to first catch `PromptTooLongError` and take the shrink path, then fall through to the existing stale-session path for other `ProcessError`s:

```
except PromptTooLongError:
    logger.error(f"{self.name} prompt too long task={task.id}; retrying with shrunk prompt + fresh session")
    await ctx.deps.session_mgr.close(task.id)
    await task_state_repo.update_state(task.id, session_id=None)
    fresh_ts = ts.model_copy(update={"session_id": None})
    # Rebuild without prior attempts (biggest accumulator on retry paths).
    req.task_context = _rebuild_task_context_without_prior(req.task_context)
    # Optional: swap to long_context_model if configured.
    if self._long_context_model_for(ctx):
        req = req.model_copy(update={"model": self._long_context_model_for(ctx)})
    session = ctx.deps.session_mgr.get_or_open(task, req, fresh_ts)
    outcome = await session.send(build_request_prompt(req))
```

Add helper `Step._long_context_model_for(ctx) -> str | None` that reads the per-step `PhaseCfg.long_context_model` and falls back to `cfg.agent.model` (i.e. `None` when neither is set).

Add helper `_rebuild_task_context_without_prior(task_context: str | None) -> str | None` that splits on the `\n\n## Previous attempts` sentinel (the header format_prior_attempts emits) and returns only the leading segment — plus a note (`> _(prior attempts elided from retry to shrink prompt)_`).

`pr_monitor.py` doesn't inject prior attempts today (it rebuilds the prompt each poll), so its retry only needs the session reset + optional model swap.

### 7. Config seam for long-context model

**File:** `src/saga/config.py`

- Extend `PhaseCfg`:
  ```
  class PhaseCfg(StrictModel):
      model: str | None = None
      long_context_model: str | None = None  # used on prompt-too-long retry
  ```
- Extend `AgentCfg`:
  ```
  class AgentCfg(StrictModel):
      model: str = "claude-sonnet-5"
      long_context_betas: list[str] = []  # e.g. ["context-1m-2025-08-07"] for Sonnet 4/4.5
  ```
- Both fields default to no-op (None / empty). No behavior change without opt-in.

**File:** `src/saga/services/claude/prompts.py`

- Extend `AgentRequest`:
  ```
  long_context_model: str | None = None
  long_context_betas: list[str] = Field(default_factory=list)
  use_long_context: bool = False
  ```

**File:** `src/saga/orchestrator/sessions.py`

- In `get_or_open`, when `req.use_long_context is True`:
  - If `req.long_context_model`, override `options_kwargs["model"]`.
  - If `req.long_context_betas`, set `options_kwargs["betas"] = list(req.long_context_betas)` (the SDK's `ClaudeAgentOptions.betas` field, `types.py:1682`).
- Default path (`use_long_context=False`): unchanged.

**File:** `docs/config-schema.md`

- Add a short subsection under "Per-phase overrides":
  ```
  ### Long-context fallback (optional)

  Each phase can name a fallback model used only when a prompt-too-long
  error is detected on the primary model — no effect on the happy path.

  triage:
    model: claude-sonnet-5
    long_context_model: claude-opus-4-5   # example; must support a larger context

  agent:
    long_context_betas: ["context-1m-2025-08-07"]   # enables Sonnet 4/4.5 1M window
  ```

**File:** `examples/linear.yaml`

- Add commented-out example lines mirroring the docs snippet, so operators can opt in with one edit.

### 8. Tests

Add / extend the following:

- **`tests/test_util.py`** (new file): `truncate_middle` — pass-through when short, head+tail preserved when long, notice contains the elided char count, works on unicode.
- **`tests/test_prompts.py`** (extend):
  - `test_format_task_context_markdown_truncates_oversized_description` — build a `LinearTaskContext` with a 40 KB description, assert output length ≤ `_MAX_DESCRIPTION_CHARS + slack`, assert head + tail both present.
  - `test_build_implementation_prompt_truncates_oversized_plan` — plan_text = 100 KB, assert output length is bounded, assert the placeholder line pointing at `plans/<identifier>.md` is present.
  - `test_format_prior_attempts_bounded` — 20 attempts each with a 1 KB detail; assert composed length ≤ `_MAX_PRIOR_ATTEMPTS_CHARS + slack`.
  - `test_response_brevity_fragment_expands` — the new shared fragment expands.
  - `test_step_prompt_contains_response_brevity` — parametrised over each step's `<step>.md`, assert `Be terse.` is present after expansion.
- **`tests/test_agent_session.py`** (extend): `test_send_raises_prompt_too_long_on_stderr_marker` — mock the SDK's `connect`/`query` to raise `ProcessError` with `stderr="…prompt is too long…"`, assert `ClaudeAgentSession.send` raises `PromptTooLongError`; also `test_send_raises_prompt_too_long_from_result_message` for the API-rejection-via-ResultMessage path.
- **`tests/test_flow_generic.py`** or a new `tests/test_step_prompt_too_long_retry.py`: mock the first `session.send` to raise `PromptTooLongError`, second to succeed; assert (a) session was reset, (b) `format_prior_attempts` output is absent from the retry prompt, (c) `long_context_model` is used when configured, (d) `long_context_betas` propagates through `ClaudeAgentOptions`.
- **`tests/test_config_phases.py`** (extend): assert `PhaseCfg.long_context_model` defaults to None and round-trips; `AgentCfg.long_context_betas` defaults to `[]`.
- **`tests/test_orchestrator_pr_review.py`** (extend): the pr_monitor retry path also treats `PromptTooLongError` distinctly.

### 9. Documentation

- `docs/config-schema.md` — new subsection (see step 7).
- `README.md` — no change (config table isn't repeated there).
- **No CLAUDE.md change.** The convention (`.claude/rules/python-style.md`) is unchanged.

---

## Edge cases and risks

- **Truncation could lose critical context.** Mitigation: head-and-tail truncation (not head-only) preserves both the goal/acceptance-criteria (top of the restructured description) and out-of-scope/related-tickets (bottom). The plan-text case additionally points the agent at the on-disk plan file — a first-class alternate path that already exists. Log a WARNING whenever any cap fires, so we can see in production whether the cap is too tight.
- **False-positive `PromptTooLongError`.** If Anthropic changes their error string, our substring match will silently miss. Mitigation: keep the marker tuple narrow (three well-known strings), and *fall through* to the existing stale-session path when it doesn't match — we never worsen the current behavior.
- **False-negative — we detect and shrink, but the shrink isn't enough.** After one shrink retry, we still fall into the normal failure path with a `mark_locally_failed`, so the ticket ends up flagged for a human just as it does today. The only difference: the failure message now says "prompt too long" instead of an opaque exit code — strictly better for the operator.
- **The long-context model may not exist / not be entitled.** Mitigation: `long_context_model` defaults to None; opt-in per phase. `long_context_betas = ["context-1m-2025-08-07"]` requires a Sonnet 4/4.5 model per SDK docs; the operator has to consciously pair the two.
- **Chat-history growth via resumed sessions is *not* directly bounded by this change.** Claude Code's own autocompact handles resumed-session compaction; the only path we control is the *incremental* single-turn prompt we send. That's the right seam — the shared/system prompt is Anthropic's, the accumulated turn history is the CLI's, and the marginal per-turn injection is Saga's. This change bounds Saga's injection; Claude's autocompact handles the rest.
- **Interaction with `_compact_state` in `task_state_store.py`.** That path already replaces `plan_text` inside the Linear-state comment with a `<saga:plan-attached ...>` placeholder when the state comment approaches Linear's 60 KB limit. When the implementation step reads the plan back via `_resolve_plan_text` (`implementation/__init__.py:91-109`), it first tries the on-disk `plans/<identifier>.md` file — so a compacted state never blocks implementation, and our new `_MAX_PLAN_TEXT_CHARS` cap can rely on the same on-disk file. No coupling change needed; the two caps work together.
- **PR-review retry site is subtler than the other two.** `pr_monitor.py` already has *two* retries (`ProcessError` on connect + failed-outcome retry). Insert `PromptTooLongError` handling at both sites, but keep the second retry a no-op when the first already reset session/model — otherwise we retry three times.
- **Test hermetics.** Every test touching `session.send` must mock the SDK (`ClaudeSDKClient.connect`/`query`) — never call the real Claude CLI. Follow the existing `tests/test_agent_session.py` pattern.

---

## Verification plan

### Before (baseline)

Numeric baseline (no code change needed to capture this — do these once so the PR description can carry the "we shrunk by N%" number):

```
uv run python -c "
from saga.schemas.task import LinearTaskContext, format_task_context_markdown
# Pull FORGE-359 description text from Linear MCP or paste the ticket text.
desc = open('/tmp/forge-359-description.md').read()
ctx = LinearTaskContext(id='x', identifier='FORGE-359', title='...', url='...', description=desc)
out = format_task_context_markdown(ctx)
print(f'description: {len(desc):,} chars')
print(f'formatted:   {len(out):,} chars')
"
```

Record these numbers in the PR description as the "before" state.

### After

- **Unit tests:** `just test` passes, including all the new tests listed in step 8.
- **Lint + types:** `just lint` passes.
- **Same numeric measurement:** rerun the snippet above with the new cap in place; assert the "formatted" length is ≤ `_MAX_DESCRIPTION_CHARS + slack` and contains the truncation notice.
- **Prompt-too-long retry simulation:** the `tests/test_step_prompt_too_long_retry.py` mock-based test IS the proof — no need for a live Claude API call.
- **Manual: verify no live-behavior regression.** In a checkout with `LINEAR_OAUTH_TOKEN` and Claude API set, run `just run`, dispatch a small-description ticket, confirm it still completes triage → plan → implementation. (Not required for CI; documented for the reviewer.)
- **Observability confirmation.** After the PR merges and one real ticket triggers the new detection, capture the `prompt-too-long detected step=... task=...` log line — that becomes the "was context window actually the binding constraint?" answer the ticket owner asked for. If it fires, the `long_context_model` opt-in is justified; if it never fires and the FORGE-359-class failures still recur, we know the culprit is elsewhere (e.g. plain retry exhaustion on a hard task) and can iterate.

### Success criteria mapping

- ✅ "Previously-failing large ticket completes without size-related failure" — verified once via the retry test + the numeric before/after; validated in prod by absence of the new log line.
- ✅ "`format_task_context_markdown` and `build_implementation_prompt` apply a documented cap/compaction strategy" — steps 2 + 3.
- ✅ "Agent instruction updated to request concise responses" — step 4.
- ✅ "Concrete finding documented on whether `claude-sonnet-5`'s context window is the binding constraint" — the plan text documents the finding as "unconfirmed at PR time; the detection log line, once it fires, is the confirmation." The `long_context_model` config knob lands ready for opt-in either way.
- ✅ "Turn-failure handling distinguishes prompt-too-long from stale-session ProcessError" — steps 5 + 6.

---

## File-by-file changelist

| File | Change |
|---|---|
| `src/saga/util.py` | Add `truncate_middle(...)`. |
| `src/saga/schemas/task.py` | Cap `ctx.description` in `format_task_context_markdown` via `truncate_middle` + `_MAX_DESCRIPTION_CHARS`. Add module logger. |
| `src/saga/services/claude/prompts.py` | Cap `plan_text` in `build_implementation_prompt` and `format_prior_attempts` per-attempt + total. Add `_MAX_PLAN_TEXT_CHARS`, `_MAX_PRIOR_ATTEMPTS_CHARS` constants. Extend `AgentRequest` with `long_context_model`, `long_context_betas`, `use_long_context`. |
| `src/saga/services/claude/session.py` | Add `_PROMPT_TOO_LONG_MARKERS`, `PromptTooLongError`. Detect in `ClaudeAgentSession.send` and `_drain`. |
| `src/saga/orchestrator/steps/step.py` | Catch `PromptTooLongError` in `GenericAgentStep.work`; add `_long_context_model_for` and `_rebuild_task_context_without_prior` helpers. |
| `src/saga/orchestrator/steps/implementation/__init__.py` | Same catch + shrink in `ImplementationStep.work`. |
| `src/saga/orchestrator/steps/review/pr_monitor.py` | Same catch + shrink at both existing retry sites. |
| `src/saga/orchestrator/sessions.py` | Propagate `req.long_context_model` and `req.long_context_betas` into `ClaudeAgentOptions` when `req.use_long_context`. |
| `src/saga/config.py` | Add `PhaseCfg.long_context_model`, `AgentCfg.long_context_betas`. |
| `src/saga/orchestrator/steps/_shared/response_brevity.md` | **New.** Concise-response instruction fragment. |
| `src/saga/orchestrator/steps/triage/triage.md` | Insert `{{shared:response_brevity}}`. |
| `src/saga/orchestrator/steps/product_definition/product_definition.md` | Insert `{{shared:response_brevity}}`. |
| `src/saga/orchestrator/steps/technical_plan/technical_plan.md` | Insert `{{shared:response_brevity}}` + plan-specific "cite as `file:line`, no big code blocks" line. |
| `src/saga/orchestrator/steps/implementation/implementation.md` | Insert `{{shared:response_brevity}}`. |
| `docs/config-schema.md` | Add long-context-fallback subsection. |
| `examples/linear.yaml` | Add commented-out `long_context_model` / `long_context_betas` example. |
| `tests/test_util.py` | **New.** `truncate_middle` tests. |
| `tests/test_prompts.py` | Add description-truncation, plan-truncation, prior-attempts-truncation, brevity-fragment tests. |
| `tests/test_agent_session.py` | Add `PromptTooLongError` detection tests. |
| `tests/test_step_prompt_too_long_retry.py` | **New.** End-to-end retry-shrink test. |
| `tests/test_config_phases.py` | Add `long_context_model` / `long_context_betas` round-trip test. |
| `tests/test_orchestrator_pr_review.py` | Extend to cover `PromptTooLongError` in pr_monitor retries. |

---

## Out of scope for this ticket (explicit)

- Compacting the resumed chat history — Claude Code's own autocompact owns that.
- Fixing FORGE-359's actual feature.
- Changing the default model.
- Reproducing the failure end-to-end against the live Claude API (the ticket owner acknowledges this is unconfirmed; the new detection log becomes the confirmation in production).
