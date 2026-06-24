# FORGE-7 — Saga session transcripts: implementation plan

## Goal

Make every agent turn produce a durable transcript file (markdown), persist it as a
`StepArtifact` attached to both Linear and Slack, replace the noisy intermediate
narration in the Slack thread with a single final summary + the transcript file, and
turn on progress narration for every step (currently only `technical_plan`).

## How we verified the project & "before-state"

Run/check commands (from `saga/` repo root, per CLAUDE.md and `justfile`):

- Tests: `just test` (or `uv run pytest`) — baseline: 696 passed in ~11.6s.
- Lint+type: `just lint` (`uv run ruff check && uv run ruff format --check && uv run ty check`) — baseline: clean.
- Single-test: `uv run pytest tests/test_<file>.py::<test>`.

The full orchestrator (`just run`) needs `LINEAR_OAUTH_TOKEN`, Slack bot/app tokens, a
GitHub App, and ngrok — not runnable in this worktree. Tests mock those boundaries, so
the test suite is the practical verification surface (also matches the project's working
agreement: "tests passing IS the verification"). Before-state notes saved to
`.saga/artifacts/forge-7-before-state.txt`.

Observed before-state (matches the ticket diagnosis):

- `sessions.py:111` `_make_on_assistant_text` returns a callback **only** for
  `technical_plan`; triage/product_definition/implementation get `None` → no
  intermediate Slack narration.
- `SessionTurnOutcome.chat_history` is populated but every caller of `session.send()`
  reads only `event/exit_code/session_id/metrics` and drops the history at the end of
  `work()`.
- `SlackNotifier` cannot delete messages and does not track posted ts'es. Intermediate
  narration messages persist forever next to the final summary.
- Linear/Slack file upload exists today only for the plan_text (via `plan_artifact.py`
  and `OutcomeReadyCtx.body` → `files_upload_v2`). No transcript artifact concept.

---

## Design overview

A new module `src/saga/services/session_transcript.py` exposes the single high-level
operation each step calls at finalisation:

```
async def publish_transcript(
    ctx: StepCtx,
    step_name: str,
    chat_history: list[Message],
    *,
    attempt: int = 1,
    status: WorkStatus,
) -> StepArtifact | None
```

It performs the full lifecycle for one turn:

1. Format `chat_history` to readable markdown (`format_session_transcript`).
2. Upload to Linear via `tracker.upload_file` + `tracker.create_attachment`
   (mirrors `plan_artifact.save_plan`).
3. Upload to Slack via `files_upload_v2` (mirrors the `OutcomeReadyCtx.body` path in
   `notifier.on_outcome_ready`).
4. Delete intermediate Slack progress messages tracked by the notifier since the
   step's `work()` began.
5. Persist a `StepArtifact(step=step_name, kind=DOCUMENT, caption="Transcript …")` onto
   `TaskState.artifacts` (appended, never overwritten — retries accumulate).
6. Return the artifact (or `None` if chat_history was empty / upload failed and the
   caller wants to log it).

Best-effort: any individual sub-step (Linear, Slack, cleanup) is wrapped in
`try/except` with `logger.exception(...)`, exactly like `plan_artifact.save_plan` and
`artifact.save_artifact`. A transcript failure never derails the step.

`chat_history` is **not** plumbed through `StepOutcome` (which would leak SDK
dataclasses into pydantic schemas). Instead, `SessionManager` gains a small cache:

```
self._last_chat_history: dict[str, list[Message]] = {}
def record_last_history(self, task_id: str, history: list[Message]) -> None
def last_chat_history(self, task_id: str) -> list[Message] | None
def pop_last_history(self, task_id: str) -> list[Message] | None  # take-and-clear
```

Each step's `work()` calls `ctx.deps.session_mgr.record_last_history(task.id,
outcome.chat_history)` immediately after `await session.send(...)`. The finaliser
pops it.

---

## File-by-file changes

### 1) New module — `src/saga/services/session_transcript.py`

Responsibilities:

- `format_session_transcript(*, identifier, step_name, attempt, started_at, chat_history, reply_text) -> str`
  — Converts a `list[Message]` to markdown. Layout:

  ```
  # Transcript — {identifier} · {step_name} · attempt {n}
  _Recorded {iso8601_utc}_

  ## Human reply (resume)
  > {reply_text}                 ← only when reply_text is not None

  ## Turn 1
  ### Prompt
  ```
  {user prompt text}
  ```
  ### Agent
  {assistant text blocks, each as a paragraph}

  ### Tool call — {ToolUseBlock.name}
  ```json
  {ToolUseBlock.input as JSON}
  ```

  ### Tool result
  ```
  {ToolResultBlock.content, truncated to ~4kB with a "...truncated" marker}
  ```

  ### Result
  - exit: {success|error_during_execution} · {n} turns · {duration_ms/1000:.1f}s
  - tokens: in / out / cached
  - cost: $X.XXXX
  ```

  Implementation: a small per-block formatter that case-matches `UserMessage`,
  `AssistantMessage`, `SystemMessage`, `ResultMessage`, `TextBlock`, `ToolUseBlock`,
  `ToolResultBlock`, `ThinkingBlock`. `SystemMessage` (`subtype="init"` etc.) is
  rendered compactly (one line) so transcripts stay readable. JSON-dump tool inputs
  with `json.dumps(..., indent=2, default=str, ensure_ascii=False)`. Truncate huge
  tool results to a configurable cap (constant `_MAX_TOOL_RESULT_CHARS = 4000`).

- `publish_transcript(...)` — orchestrates upload + cleanup as described above.

  Filename: `{identifier}-{step_name}-attempt{n}-{YYYYMMDDhhmmss}.md`
  Caption (StepArtifact + attachments): `Transcript — {step_name} attempt {n}`
  Content-type for `tracker.upload_file`: `text/markdown` (same as `upload_markdown`).
  Slack upload uses `files_upload_v2(channel=…, thread_ts=…, content=…, filename=…, title=…)`.
  Fallback when Slack upload fails and a Linear asset URL exists: post a chat-message
  link to the Linear-hosted transcript (mirrors `notifier.on_outcome_ready`'s plan
  fallback).

  Side effects, in order:
  1. `tracker.upload_file(filename, "text/markdown", text.encode())`
  2. `tracker.create_attachment(task_id, caption, asset_url)`
  3. `notifier.upload_transcript(channel, thread_ts, filename, text, fallback_url)`
     (new thin wrapper on SlackNotifier — see §3 below)
  4. `notifier.flush_step_progress(task_id)` — deletes intermediate progress
     messages tracked since the step started.
  5. Append `StepArtifact(step=step_name, kind=StepArtifactKind.DOCUMENT,
     caption=caption, linear_attachment_id=…, linear_asset_url=…)` to
     `TaskState.artifacts` via `task_state_repo.update_state(task_id,
     artifacts=[*ts.artifacts, new])`.

  Returns the `StepArtifact` (or None if `chat_history` is empty after stripping the
  init `SystemMessage` — nothing to record).

Style/lint adherence (from `.claude/rules/python-style.md`): type everything, module
logger `logging.getLogger("saga.services.session_transcript")`, no untyped dicts at
boundaries, async-first, no dataclasses, f-strings.

### 2) `src/saga/orchestrator/sessions.py` — enable narration for every step + cache chat history

- Replace `_make_on_assistant_text(...)` so the narration callback is wired for every
  agent step that has a Slack thread, not just `technical_plan`:

  ```
  if state_name not in ("triage", "product_definition", "technical_plan", "implementation"):
      return None
  ```

  `pr_review` is intentionally excluded — its poll is a no-op on most ticks and it
  isn't a step the human watches narrated.

- The callback's body posts via the new `notifier.post_step_progress(task_id, …)`
  instead of `post_agent_message` so the ts is tracked for later cleanup (see §3).

- Add `self._last_chat_history: dict[str, list[Message]] = {}` to
  `SessionManager.__init__`. Add methods:

  ```
  def record_last_history(self, task_id: str, history: list[Message]) -> None:
      self._last_chat_history[task_id] = history
  def pop_last_history(self, task_id: str) -> list[Message] | None:
      return self._last_chat_history.pop(task_id, None)
  ```

  Wipe entries in `close(task_id)` and `close_all()` so a closed session doesn't
  retain history across reincarnations.

### 3) `src/saga/services/slack/notifier.py` — message-id tracking, delete, transcript upload, new progress method

- Add to `SlackNotifier.__init__`:

  ```
  self._step_progress_messages: dict[str, list[str]] = {}  # task_id → list of ts'es
  ```

- New method:

  ```
  async def post_step_progress(self, task_id: str, channel_id: str, thread_ts: str, text: str) -> str | None:
      """Post a step-progress narration message and track its ts for later cleanup."""
  ```

  Implementation: same body as `post_agent_message` (chunked via `plan_blocks`,
  ≤200-char text fallback) but records `resp["ts"]` into
  `self._step_progress_messages.setdefault(task_id, []).append(ts)`. Returns the
  ts or None on Slack error (logged and swallowed — narration must not derail work).

  `post_agent_message` keeps its current signature (no return value) and is used by
  callers that should NOT be cleaned up: `pr_monitor`, `terminal/aggregate`,
  implementation's verification-artifacts summary.

- New method:

  ```
  async def flush_step_progress(self, task_id: str) -> int:
      """Delete every tracked intermediate progress message for ``task_id`` and clear
      the tracker. Returns the number of deletions attempted."""
  ```

  Iterates `self._step_progress_messages.pop(task_id, [])`, calling
  `self._web.chat_delete(channel=self._channel_id, ts=ts)`. Each delete is wrapped in
  `try/except SlackApiError` with `logger.exception` — failures (e.g. message older
  than retention, missing permission) are logged and skipped.

- New method:

  ```
  async def upload_transcript(
      self,
      channel_id: str,
      thread_ts: str,
      filename: str,
      content: str,
      fallback_url: str | None,
  ) -> None:
  ```

  Mirrors the existing `files_upload_v2` block at notifier.py:349 (the plan-upload
  path); on `SlackApiError`, falls back to a clickable link to `fallback_url` (the
  Linear asset URL).

- Optional but small win: thread the channel through `post_step_progress` and
  `post_agent_message` rather than relying on `self._channel_id` (current code does
  both inconsistently). Out of scope for FORGE-7 — only `post_step_progress` is new
  here.

### 4) Step `work()` methods — record chat_history after every turn

Only one line change at each site, immediately after `outcome = await session.send(prompt)`:

  ```
  ctx.deps.session_mgr.record_last_history(task.id, outcome.chat_history)
  ```

Sites:

- `src/saga/orchestrator/steps/step.py` — `GenericAgentStep.work()` (covers
  triage, product_definition, technical_plan).
- `src/saga/orchestrator/steps/implementation/__init__.py` — `ImplementationStep.work()`.

`pr_review` does not call `session.send` directly in its own `work()`; the per-turn
fix path inside `PrMonitor.poll` uses sessions, and per ticket scope (success
criterion #1 says "every step that runs with a workdir"), pr_review is **excluded**.

### 5) Finalisation hooks — call `publish_transcript` from each step's success/failure path

A single helper to keep the call-site small. Define inside
`session_transcript.py`:

```
async def publish_for_step(
    ctx: StepCtx,
    step_name: str,
    *,
    status: WorkStatus,
) -> StepArtifact | None:
    """Pop the last history off SessionManager and call publish_transcript."""
```

It also computes the attempt number (`len([r for r in ts.step_records if r.step == step_name]) + 1`)
so retries are labelled.

Wire it from:

- **`generic.py::publish_outcome`** — after the `on_outcome_ready` call returns
  (covers `technical_plan` and `implementation` success paths).
- **`triage/__init__.py::TriageStep.post_step`** — after the inline
  `on_outcome_ready` block (happy path only; needs-human paths funnel through
  `needs_human()` which does its own Slack post — see §6 for that path).
- **`product_definition/__init__.py::ProductDefinitionStep.post_step`** — after
  the inline `on_outcome_ready` block.
- **`generic.py::on_failure`** — after the failure `StepRecord` is appended.
  This handles both intermediate failures (retry attempt) and terminal failures
  (`mark_locally_failed`). The transcript captures everything the agent did before
  failing.
- **`generic.py::record_verifier_fail`** — same hook for verifier-driven retries
  (the chat_history of the agent turn the verifier rejected should still be saved).

In every site, call:

```
await publish_for_step(ctx, step.name, status=out.status)
```

Order: the transcript publish always runs **after** the final summary message is
posted to Slack, so the in-thread order is:

1. (intermediate narration messages — now deleted by `flush_step_progress`)
2. Final summary (`on_outcome_ready`)
3. Transcript file (`files_upload_v2`)

### 6) Approval-reply capture

The reply already lands in chat_history naturally on the next `work()` turn —
`GenericAgentStep.work` (step.py:266) uses `ctx.reply_text` as the first user prompt,
which becomes the next `UserMessage` in `chat_history`. So when the next step's
turn completes, its transcript already contains the reply.

Additionally, `format_session_transcript` accepts `reply_text` and renders a
prominent "Human reply (resume)" block at the top of the transcript so a reader
sees the human's text without reading the whole prompt. `publish_for_step` reads
`ctx.reply_text` and passes it through.

### 7) Retries / failed-attempt retention

`publish_transcript` always appends a new `StepArtifact` and creates a new Linear
attachment per attempt (no replace, unlike `plan_artifact.save_plan` which deletes
the prior attachment). The filename has `attempt{n}` and a timestamp suffix, so two
attempts of the same step never collide on either platform. `TaskState.artifacts`
keeps the full history (already append-only).

### 8) Tests

New test file `tests/test_session_transcript.py`:

- `test_format_session_transcript_basic` — given a synthetic UserMessage +
  AssistantMessage[TextBlock] + ResultMessage chain, the produced markdown contains
  the expected headers ("Prompt", "Agent", "Result"), the prompt body, and the
  assistant text.
- `test_format_session_transcript_tool_calls` — ToolUseBlock + ToolResultBlock are
  rendered with the tool name, JSON-formatted input, and the truncated result.
- `test_format_session_transcript_includes_reply_text` — when `reply_text` is
  passed, the "Human reply (resume)" block appears above the first turn.
- `test_format_session_transcript_empty_history_returns_none` — `publish_transcript`
  returns None and skips uploads when chat_history is empty (or contains only
  init SystemMessage).
- `test_publish_transcript_uploads_to_linear_and_slack` — mock `tracker.upload_file`,
  `tracker.create_attachment`, `notifier.upload_transcript`, and
  `notifier.flush_step_progress`. Verify all four are awaited in order, and a
  `StepArtifact` is appended to TaskState.artifacts.
- `test_publish_transcript_continues_when_linear_upload_fails` — make
  `tracker.upload_file` raise; verify Slack upload still runs (best-effort
  semantics, mirrors `plan_artifact.save_plan` and `artifact.save_artifact`).
- `test_publish_transcript_continues_when_slack_upload_fails` — symmetric.
- `test_publish_transcript_attempt_number_increments` — when prior step_records
  exist for the same step, attempt is `n+1`.

Augment `tests/test_notify_slack.py`:

- `test_post_step_progress_tracks_ts` — call `post_step_progress`, assert the ts
  is appended to `self._step_progress_messages[task_id]`.
- `test_flush_step_progress_deletes_messages` — pre-populate the tracker, mock
  `web.chat_delete`, assert it was awaited once per ts and the list is cleared.
- `test_flush_step_progress_continues_on_delete_error` — `web.chat_delete` raises
  SlackApiError; `flush_step_progress` still clears the tracker.
- `test_upload_transcript_uses_files_upload_v2` — happy path.
- `test_upload_transcript_falls_back_to_linear_link_on_error` — `files_upload_v2`
  raises; a chat_postMessage with the fallback URL is sent.

Augment `tests/test_orchestrator_session_lifecycle.py` (or add to
`test_agent_session.py`):

- `test_session_manager_records_last_history_per_task` — assert
  `SessionManager.record_last_history` then `pop_last_history` round-trips a
  chat history list.
- `test_session_manager_clears_history_on_close` — closing the session also
  pops the cached history.
- `test_make_on_assistant_text_returns_callback_for_every_named_step` —
  triage / product_definition / technical_plan / implementation get a callback;
  pr_review gets None.

Augment `tests/test_flow_generic.py`:

- `test_publish_outcome_publishes_transcript_after_on_outcome_ready` — using
  the existing fixture pattern, patch `session_transcript.publish_for_step` and
  assert it is awaited exactly once after `on_outcome_ready`.

Augment `tests/test_triage_step.py` and `tests/test_product_definition.py`:

- `test_post_step_happy_path_publishes_transcript` — patch
  `session_transcript.publish_for_step`, assert it's called.

Augment `tests/test_implementation_step.py`:

- `test_implementation_records_chat_history` — assert
  `session_mgr.record_last_history` is called after `session.send`.

Augment `tests/test_flow_runner.py` / `test_flow_generic.py` for the failure path:

- `test_on_failure_publishes_transcript` — when `on_failure` runs, the
  transcript helper is awaited (capturing pre-failure history).

---

## Order of changes (dependencies first)

1. Add `SessionManager.record_last_history` / `pop_last_history` and the dict
   field (sessions.py). No callers yet — safe to land.
2. Add `SlackNotifier.post_step_progress`, `flush_step_progress`,
   `upload_transcript` and the `_step_progress_messages` dict. Unit-test in
   isolation. No callers yet.
3. Add `src/saga/services/session_transcript.py` with
   `format_session_transcript`, `publish_transcript`, `publish_for_step`.
   Unit-test in isolation.
4. Switch `sessions.py::_make_on_assistant_text` to: (a) cover every named step,
   (b) use `notifier.post_step_progress` instead of `post_agent_message`.
5. Insert `session_mgr.record_last_history(task.id, outcome.chat_history)` in
   `GenericAgentStep.work()` (step.py) and `ImplementationStep.work()`
   (implementation/__init__.py).
6. Hook `publish_for_step` into:
   - `generic.py::publish_outcome` (after `on_outcome_ready`)
   - `triage/__init__.py::TriageStep.post_step` (after `on_outcome_ready`)
   - `product_definition/__init__.py::ProductDefinitionStep.post_step` (after `on_outcome_ready`)
   - `generic.py::on_failure` (after the failure record is appended)
   - `generic.py::record_verifier_fail` (after the failure record is appended)
7. Update existing tests that previously expected `on_assistant_text=None` for
   non-technical_plan states (search for `_make_on_assistant_text` in
   `tests/test_orchestrator_session_lifecycle.py` and adjust).
8. Add the new tests listed in §8.
9. `just lint-fix && just lint && just test` — must be clean.

---

## Edge cases & risks

- **Slack `chat.delete` permission** — the bot needs `chat:write` (already present
  for posting) plus permission to delete its own messages, which Slack grants for
  bot-authored messages by default. If `chat.delete` returns
  `message_not_found` / `cant_delete_message`, log and continue; the failure is
  benign (worst case the intermediate message stays). Document this in the
  module docstring so an operator who notices stale narration knows what scope
  to check.

- **Slack `files:write` scope** — already required for the existing plan upload;
  notifier.py:357 already documents the requirement. We reuse the same path.

- **Linear attachment growth** — each retry adds a transcript attachment. A
  ticket that retries many times accumulates attachments. Acceptable trade-off
  for audit/learning; explicitly called out in success criterion #6. Not deleting
  prior-attempt attachments is *intentional*, unlike `plan_artifact.save_plan`'s
  replace-on-update model.

- **`pr_review` step exclusion** — pr_review's `work()` is a poll, not a turn; the
  inline fix turn inside `PrMonitor.poll` could also benefit from transcripts, but
  it's out of the strict ticket scope ("every step that runs with a workdir" — the
  pr_review *step* has `eager_workspace=False`). Noted as a follow-up rather
  than scope creep.

- **No-thread runs** — if `ts.notifier is None` (Slack disabled or thread not yet
  posted), Slack upload + flush are no-ops. Linear attachment still runs.

- **No-history runs (e.g. work() FAILED before sending)** — `chat_history` is
  empty; `publish_transcript` returns None and writes nothing. Safe.

- **Race with `resets_session_on_advance`** — `advance()` calls
  `session_mgr.close(task.id)`. Our hook into `publish_outcome` runs *before*
  `advance()` in the success path (publish_outcome → caller decides to advance),
  so the history is popped and the transcript uploaded before `close()` wipes
  the cache. We MUST `pop` (not `get`) in `publish_for_step` so a follow-up
  re-entry doesn't see stale history.

- **`update_state(artifacts=...)` race** — multiple concurrent updates to
  `TaskState.artifacts` are already serialised by the per-task asyncio lock in
  `task_state_store`. No new locking needed.

- **Transcript size / Slack file limit** — Slack's file upload limit (`files.upload`
  bound) is ~1GB; transcripts at worst few MB. We hard-cap each tool-result block
  at 4kB in `format_session_transcript` so a 1k-tool-call turn stays bounded.

- **Tests touching `on_assistant_text` defaults** — turning narration on for every
  step may flip an existing test that asserted no narration for triage/PD/impl.
  `tests/test_orchestrator_session_lifecycle.py` is the likely candidate;
  audit and update those assertions in step 7 above.

---

## Verification (after the change)

Re-run from the repo root:

- `just lint` — must remain green.
- `just test` — all 696 existing tests still pass plus the new tests under
  `tests/test_session_transcript.py` (~7 cases) and the augmentations.
- Spot-check by reading: in `tests/test_flow_generic.py`, the new
  `test_publish_outcome_publishes_transcript_after_on_outcome_ready` confirms
  the order. In `tests/test_notify_slack.py`, the new
  `test_flush_step_progress_deletes_messages` confirms cleanup.
- Save an after-state note to `.saga/artifacts/forge-7-after-state.txt` summarising
  the same five aspects from the before-state file, contrasting before/after, so
  the orchestrator can upload it.

Full end-to-end (requires real services, **out of this environment's reach**):
manually trigger a triage/product_definition/technical_plan/implementation run
and verify a thread file `Transcript — <step> attempt 1.md` appears for each,
the intermediate narration messages are gone, and a matching Linear attachment
exists on the issue.
