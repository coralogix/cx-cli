# FORGE-551 — Implementation plan: shrink context + self-heal on context-window hits

Two levers, both required by the ticket. Part A activates the already-built recovery path in prod; Part B adds a saga-owned mechanism that proactively shrinks per-turn context for long-lived sessions.

## Before-state (code-grounded; see `.saga/artifacts/forge-551-before-state.md`)
This bug only reproduces with a real ~200k-token production session (FORGE-181); it can't be reproduced locally. Verified by reading code:
1. **Recovery dormant in prod:** `deploy/config.yaml` sets `model:` per phase but never `long_context_model:`; `PhaseCfg.long_context_model` defaults `None` (config.py:100). So `Step._retry_with_long_context_model` (step.py:244) and `PrMonitor._retry_with_long_context_model` (pr_monitor.py:449) early-return `None` → every `PromptTooLongError` fails closed (step.py:446-458, implementation/__init__.py:371-383, pr_monitor.py:559-570).
2. **No bound on accumulated session:** implementation keeps the session open for pr_review (implementation/__init__.py:464-467); `PrMonitor._dispatch_turn` resumes it every tick (pr_monitor.py:533) and re-persists `session_id` (pr_monitor.py:593). The CLI's resumed transcript grows unbounded; saga has no trim/threshold on it (`_MAX_TOOL_RESULT_CHARS` at session_transcript.py:36 only truncates the rendered artifact). FORGE-455 capped only the three injected text blocks.

## Part A — Activate the recovery path in production (Part 2 of the ticket)

**A1. `deploy/config.yaml`** — set `long_context_model` on the phases that hit the wall, plus the beta flag:
- Under `agent:` add `long_context_betas: ["context-1m-2025-08-07"]` (Anthropic 1M-context beta; `AgentCfg.long_context_betas: list[SdkBeta]`).
- Set `long_context_model: <1M-context alias>` on `implementation:`, `technical_plan:`, `product_definition:`. Setting `implementation.long_context_model` also covers pr_review, which reuses the implementation phase cfg (pr_monitor.py:449).
- **OPEN CONFIG VALUE:** the repo uses custom gateway aliases (`claude-sonnet-5`, `claude-opus-4-8`); the exact 1M-context alias + beta string must be confirmed by an operator against the gateway. Use the values above as documented placeholders and call this out in the PR description so an operator verifies before/at deploy. The escalation opens a fresh resume-less session on the fallback model (step.py:254-262), so it works regardless of which valid alias is chosen.

No code change is needed for A — the escalation path already exists and is tested (step.py:232-268, implementation/__init__.py:241-242/366-383, pr_monitor.py:439-472). This is config-only, but it is the change that stops the dead-end-to-needs-human behavior.

## Part B — Proactively shrink long-lived session context (Part 1 of the ticket)

Saga doesn't own the CLI's resumed transcript, so the saga-owned lever is to **reset an oversized session** (close it + clear `session_id`) after a turn whose context grew past a configurable threshold, so the *next* turn starts fresh. This is the same recovery the codebase already uses on a stale/crashed session (implementation/__init__.py:355-363, pr_monitor.py:541-550) — just triggered by token volume instead of a crash. The plan is re-injected at implementation kickoff and pr_review prompts are self-contained per tick (comments/CI/conflict), so a fresh session mid-lifecycle is safe and loses no authoritative input.

**B1. Config knob — `config.py`.** Add to `AgentCfg`:
```
session_reset_context_tokens: int | None = None
```
Opt-in (default `None` = disabled), matching FORGE-455's conservative philosophy. One-line `#` comment: when a completed heavy turn's context (input + cache-read + cache-creation tokens) meets/exceeds this, saga resets the session so the next turn starts fresh.

**B2. Helper + primitive — `orchestrator/sessions.py`.**
- Add module function `context_tokens(metrics: StepMetrics | None) -> int | None`: sum of `input_tokens`, `cache_read_tokens`, `cache_creation_tokens` (treat `None` as 0; return `None` when all three are `None`). Import `StepMetrics` from `saga.schemas.step` (already used indirectly). These are exactly the fields populated by `_metrics_from_result` (session.py:93-139).
- Add `SessionManager.reset_if_oversized(self, task_id: str, metrics: StepMetrics | None, threshold: int | None) -> bool`: return `False` when `threshold is None`, `metrics is None`, or `context_tokens < threshold`; otherwise `logger.warning(...)`, `await self.close(task_id)`, `await task_state_repo.update_state(task_id, session_id=None)`, return `True`. (`task_state_repo` is already imported in sessions.py.)

**B3. Call sites — one per heavy-turn lifecycle, each placed *after* the success-path `session_id` persist and *before* the session is carried forward.** Read the threshold from `ctx.cfg.agent.session_reset_context_tokens` / `self._cfg.agent.session_reset_context_tokens`.
- **`orchestrator/steps/step.py` `GenericAgentStep.work`** — at the very end, after `_capture_structured_result` and just before the final `return StepOutcome(DONE...)` (step.py:600-602). Placed after capture so the extraction turn (which reuses the same session, step.py:529) still runs. These steps' `post_step` (triage/product_definition/technical_plan) do not re-write `session_id`, so the reset sticks; the next phase opens fresh.
- **`orchestrator/steps/implementation/__init__.py` `ImplementationStep.post_step`** — inside the `if review_enabled:` branch that currently logs "Session stays open" (implementation/__init__.py:464-467). `post_step` re-writes `session_id` from `out.session_id` at line 400, so the reset MUST go after that (this branch is after it). Call `reset_if_oversized(task.id, out.metrics, threshold)`; when it resets, log that pr_review will start fresh. First pr_review tick then opens a fresh session (its `get_or_open` reads `ts.session_id`, now `None`). Do NOT reset in `work()` — line 400 would clobber it.
- **`orchestrator/steps/review/pr_monitor.py` `PrMonitor._dispatch_turn`** — immediately after `await task_state_repo.update_state(task.id, session_id=outcome.session_id)` (pr_monitor.py:593). Reset using `outcome.metrics`; next fix tick opens fresh.

`StepOutcome` already carries `metrics` (populated from the turn), so no signature changes are needed.

## Order of changes (dependencies first)
1. B1 (`AgentCfg` field) → 2. B2 (helper + `reset_if_oversized`) → 3. B3 (three call sites) → 4. A1 (`deploy/config.yaml`: `long_context_betas` + per-phase `long_context_model`; also add `agent.session_reset_context_tokens: 150000` to turn B on in prod) → 5. tests → 6. `just lint && just test`.

Suggested prod value: `agent.session_reset_context_tokens: 150000` (well under Sonnet's ~200k window, leaving headroom for the injected plan + a turn's output). Tunable.

## Edge cases / risks
- **Reset loses accumulated context.** Mitigated: implementation re-injects the full plan (implementation/__init__.py:330-336); pr_review prompts are self-contained per tick; the diff lives on the PR branch which the fix turn re-fetches. This is already the accepted trade-off on the stale-session fresh path.
- **Clobber ordering (implementation).** `post_step` writes `session_id` at line 400 — reset must run after it (B3 places it in the later `review_enabled` branch). Covered by a test asserting `session_id` ends `None`.
- **Structured-capture reuse (GenericAgentStep).** Reset must run after `_capture_structured_result`, never between the main and extraction turns — B3 places it at the end of `work`.
- **Threshold default `None`.** With the knob unset, behavior is unchanged everywhere (safe for the shared `AgentCfg`); prod opts in via A1.
- **Metrics absent.** `reset_if_oversized` no-ops when `metrics is None` or all token fields are `None` — never resets on missing data.
- **`long_context_model` alias unverified.** Flagged as an operator check in the PR; wrong alias would surface as a fresh escalation failure (already logged, fails closed — no worse than today).

## Tests (mirror the FORGE-455 pattern in tests/test_step.py, test_implementation_step.py, test_orchestrator_pr_review.py, test_config_phases.py)
- **`tests/test_config_phases.py`** (or test_config.py): `AgentCfg.session_reset_context_tokens` defaults `None`; parses an int from YAML.
- **`tests/test_orchestrator_sessions.py`** (SessionManager): `context_tokens` sums the three fields / handles `None`; `reset_if_oversized` — (a) below threshold → no `close`, `session_id` unchanged; (b) `threshold=None` → no-op; (c) at/above threshold → `close` awaited + `update_state(session_id=None)` + returns `True`.
- **`tests/test_step.py`**: `GenericAgentStep.work` with metrics over threshold → session closed and state `session_id is None` after work; under threshold → session retained. Existing long-context escalation tests (test_step.py:340-401) must still pass.
- **`tests/test_implementation_step.py`**: `post_step` in the review-enabled branch resets when `out.metrics` exceeds threshold (state `session_id is None`, session closed) and does NOT reset under threshold (existing "session stays open" behavior preserved — test_orchestrator_pr_review.py:206 style).
- **`tests/test_orchestrator_pr_review.py`**: `_dispatch_turn` resets the session when the turn's metrics exceed threshold; unchanged when under.

## Verification
- Commands (from CLAUDE.md / justfile): `just lint` (ruff + ruff format --check + ty) and `just test` (pytest). Single file: `uv run pytest tests/test_orchestrator_sessions.py`.
- Behavior to observe: **before** — a `PromptTooLongError` in the implementation/pr_review path with no `long_context_model` fails closed to needs-human (existing tests test_step.py:318-338, test_implementation_step.py:569-596 encode this). **after** — (Part A) with `long_context_model` configured, the same error triggers one escalation retry that completes (test_step.py:340-372 encodes the mechanism; A is config so validate via the config-loaded escalation test); (Part B) a turn reporting context tokens over the threshold causes the next turn to open a fresh (resume-less) session, asserted via the new reset tests.
- Runtime repro of the original 200k-token failure is not feasible locally; correctness is verified by the regression tests that simulate the oversized turn/metrics, per the ticket's success criteria.
