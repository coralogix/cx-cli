# FORGE-181 — Ship Saga logs & traces to Coralogix (OTLP)

## Summary of current state (verified)
- `src/saga/logging.py` — `RichHandler` (stderr) + JSON `FileHandler`; nothing ships externally.
- `pyproject.toml:12` — only `opentelemetry-api`; no SDK, no exporter, no `TracerProvider`, no spans.
- `src/saga/settings.py` — no Coralogix/OTEL fields.
- `agent_env()` (`services/claude/prompts.py:157`) — scrubs secrets, injects no OTEL vars.
- Entry: `entrypoints/cli/main.py:run_orchestrator` calls `init_logging(cfg.logging)` then `serve()` (`bootstrap.py`).
- Spawn sites all funnel through `SessionManager.get_or_open` (`orchestrator/sessions.py:190`), which calls `agent_env(req)`.
- Baseline `just test` / `tests/test_agent_env.py` green.

## Design decisions
- **One OTLP HTTP/protobuf stack** for both traces and logs → `opentelemetry-exporter-otlp-proto-http` (pulls in `opentelemetry-sdk`). HTTP (not gRPC) is simpler through proxies and matches Claude Code's default `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf`.
- **Fail-open / opt-in:** when `CORALOGIX_OTLP_ENDPOINT`/`CORALOGIX_API_KEY` are unset, no provider is installed, no log handler is attached, and `agent_env` injects no OTEL vars — local dev and tests run unchanged. Adding spans is always safe: `trace.get_tracer(...)` returns a no-op tracer with no provider.
- **Coralogix specifics:** OTLP ingest needs `Authorization: Bearer <api-key>` header and resource attributes `cx.application.name` + `cx.subsystem.name`. Defaults: application `saga`, subsystem `orchestrator`.
- **Claude Code correlation:** Claude Code's OTEL emits **metrics + log events** (not internal traces). Criterion 3's "child span in the same trace, OR a correlated trace ID" is satisfied by injecting `OTEL_RESOURCE_ATTRIBUTES` carrying `task.id` (+ service name) so its telemetry is queryable alongside saga's, plus optional `traceparent` injection for context linkage. This is called out honestly rather than promising literal child spans of saga's pipeline.

## Changes, in dependency order

### 1. Dependencies — `pyproject.toml`
- Replace the lone `opentelemetry-api>=1.33.1` with:
  - `opentelemetry-sdk>=1.33.1`
  - `opentelemetry-exporter-otlp-proto-http>=1.33.1`
  (`opentelemetry-api` comes transitively; keep an explicit pin if ruff/ty complain.)
- Run `uv sync` to refresh `uv.lock` (commit the lock).

### 2. Settings — `src/saga/settings.py`
Add env-only fields (all optional so absence disables export):
- `CORALOGIX_OTLP_ENDPOINT: str | None = None` (e.g. `https://ingress.eu2.coralogix.com`)
- `CORALOGIX_API_KEY: str | None = None`
- `CORALOGIX_APPLICATION_NAME: str = "saga"`
- `CORALOGIX_SUBSYSTEM_NAME: str = "orchestrator"`
Add a helper `coralogix_enabled(self) -> bool` returning `bool(endpoint and api_key)`.

### 3. New module — `src/saga/observability/telemetry.py`
Central, idempotent OTEL wiring. Pure functions, guarded by settings; no work when disabled. Public surface:
- `build_resource(settings) -> Resource` — sets `service.name=saga`, `cx.application.name`, `cx.subsystem.name`, `cx.subsystem.name`, and any `service.version`.
- `otlp_headers(settings) -> dict[str,str]` → `{"Authorization": f"Bearer {api_key}"}`.
- `configure_tracing(settings) -> None` — idempotent (module-level `_tracing_ready` guard); when enabled, installs a `TracerProvider(resource=...)` with `BatchSpanProcessor(OTLPSpanExporter(endpoint=<endpoint>/v1/traces, headers=...))` via `trace.set_tracer_provider`. No-op when disabled.
- `build_log_handler(settings) -> logging.Handler | None` — when enabled, builds a `LoggerProvider(resource=...)` with `BatchLogRecordProcessor(OTLPLogExporter(endpoint=<endpoint>/v1/logs, headers=...))` and returns an `opentelemetry.sdk._logs.LoggingHandler(level=..., logger_provider=...)`; `None` when disabled.
- `claude_otel_env(task_id, settings) -> dict[str,str]` — returns `{}` when disabled, else the env block for Claude Code child processes (see §7).
- `shutdown() -> None` — flush + shut down both providers (called on graceful exit).
- Module logger `saga.observability.telemetry`; any exporter-init failure is caught, logged via `logger.exception`, and degrades to disabled (observability must never crash the orchestrator).

### 4. Log shipping — `src/saga/logging.py`
- After the existing `logging.config.dictConfig(...)`, call `telemetry.build_log_handler(get_settings())`; if non-`None`, `logging.getLogger().addHandler(handler)` at the configured `level`. Keep console + file handlers unchanged (Coralogix is additive).
- Guard the `get_settings()` call so a partial dev env can't break stderr logging: wrap in try/except and log a warning on failure (mirrors the existing `file_error` fallback). `init` is only invoked from `saga run`, where full env is present.

### 5. Tracer bootstrap — `src/saga/bootstrap.py`
- At the top of `serve()` (before building services), call `telemetry.configure_tracing(settings)` (settings already fetched at line 41).
- After `run_http_server(...)` returns (line 97), call `telemetry.shutdown()` in a `finally` so spans/logs flush on shutdown.

### 6. Span boundaries
Instrument the three ticket-named boundaries with `tracer = trace.get_tracer("saga.<area>")` and `with tracer.start_as_current_span(name, attributes={...}) as span:`. All no-ops when tracing disabled.
- **Task dispatch** — `orchestrator/loop.py` `_dispatch_step()` (the per-task background entry): span `saga.dispatch`, attrs `task.id`, `task.identifier`, `phase`, `step`. This is the pipeline root per task-turn.
- **Step run** — `orchestrator/steps/runner.py` `StepRunner.run()`: span `saga.step`, attrs `task.id`, `step.name`, `stage`; record exceptions via `span.record_exception` + `span.set_status(StatusCode.ERROR)` in the existing except blocks (don't change control flow).
- **Claude turn** — `services/claude/session.py` `ClaudeAgentSession.send()`: span `saga.claude_turn`, attrs `session.resume` (connected?) and, on completion, `exit_code` + token metrics from `SessionTurnOutcome.metrics` (input/output/total tokens, cost). Set error status on `event=="failed"` / raised `ProcessError`/`PromptTooLongError`.
- Keep spans minimal and attribute-driven; do not restructure the async task model. Additional spans beyond these are out of scope for v1 (ticket delegates "additional coverage" but names these three).

### 7. Claude Code instrumentation — `agent_env()` (`services/claude/prompts.py`)
- Merge `telemetry.claude_otel_env(request.task_id, get_settings())` into the returned dict **before** `**request.extra_env` (so explicit `extra_env` still wins; matches existing precedence tested by `test_agent_env_extra_env_can_override_scrubbed_value`).
- The env block (only when Coralogix enabled):
  - `CLAUDE_CODE_ENABLE_TELEMETRY=1`
  - `OTEL_EXPORTER_OTLP_ENDPOINT=<endpoint>`
  - `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf`
  - `OTEL_EXPORTER_OTLP_HEADERS=Authorization=Bearer%20<api-key>,...` (URL-encode per OTEL spec)
  - `OTEL_LOGS_EXPORTER=otlp`, `OTEL_METRICS_EXPORTER=otlp`, `OTEL_TRACES_EXPORTER=otlp`
  - `OTEL_SERVICE_NAME=saga-claude`
  - `OTEL_RESOURCE_ATTRIBUTES=cx.application.name=<app>,cx.subsystem.name=claude-code,task.id=<task_id>`
  - `OTEL_PROPAGATORS=tracecontext,baggage`
  - (optional) `TRACEPARENT` from the current active span context so Claude telemetry links to the saga trace.
- **Security note:** unlike GitHub/Linear/Slack secrets (scrubbed), the Coralogix key is intentionally passed to the child — it's a write-only ingest key required for the child to export. Document this in a comment beside the injection, and do NOT add it to the scrub list.

### 8. Deploy / config plumbing
- `deploy/docker-compose.yml` — the service already uses `env_file: .env`, so `CORALOGIX_*` vars flow through automatically; no compose change strictly required. Optionally add them to the `environment:` block for documentation.
- `.env.example` — add commented `CORALOGIX_OTLP_ENDPOINT=`, `CORALOGIX_API_KEY=`, `# CORALOGIX_APPLICATION_NAME=saga`, `# CORALOGIX_SUBSYSTEM_NAME=orchestrator`.
- No secrets committed; `deploy/config.yaml` unchanged (secrets stay in env, matching existing `GITHUB_APP_*` pattern).

### 9. Tests (unit; no live Coralogix)
- `tests/test_telemetry.py` — with settings unset: `configure_tracing` is a no-op, `build_log_handler` returns `None`, `claude_otel_env` returns `{}`. With fake endpoint+key (monkeypatch `get_settings`): handler is a `LoggingHandler`, `claude_otel_env` contains the expected keys and the URL-encoded auth header, resource attrs include `task.id`.
- Extend `tests/test_agent_env.py` — (a) unset → no `OTEL_*`/`CLAUDE_CODE_ENABLE_TELEMETRY` keys; (b) configured → keys present and `task.id` matches; (c) `extra_env` still overrides.
- `tests/test_logging_cli.py` or new `tests/test_logging_export.py` — `init(cfg)` with Coralogix unset attaches no OTLP handler and doesn't raise.
- Span tests: use OTEL's `InMemorySpanExporter` + `TracerProvider` to assert `StepRunner.run` / `ClaudeAgentSession.send` emit a span with expected name/attributes; assert error status on the failure path. Keep existing tests green (spans are no-ops without a provider).

## Edge cases / risks
- **Never crash on telemetry failure:** all exporter init and shutdown wrapped in try/except + `logger.exception`; disabled fallback.
- **Batch flush on shutdown:** `telemetry.shutdown()` in `serve()`'s `finally` prevents losing buffered spans/logs when the orchestrator stops (30s grace in compose).
- **`_logs` module is OTEL-"private"** but stable and standard; if churn is a concern, pin the exporter/sdk minor. Acceptable.
- **Log volume / cost:** root handler at configured level (default info) mirrors the file handler — no new verbosity. httpx/httpcore stay WARNING (existing dictConfig).
- **Key exposure to child process** is intentional and limited to a write-only ingest key; documented.
- **ty/ruff:** OTEL SDK is typed; if `_logs.LoggingHandler` import trips ty, import from `opentelemetry.sdk._logs` and add a narrow, commented ignore only if genuinely bogus.

## Verification
Run/check commands (from `justfile`, per `CLAUDE.md`):
- Lint+types: `just lint` (`ruff check` + `ruff format --check` + `ty check`).
- Tests: `just test`; scoped: `uv run pytest tests/test_telemetry.py tests/test_agent_env.py tests/test_logging_export.py -q`.
- Run: `just run` (`uv run saga run`) — needs full secret env.

Before/after behavior:
- **Before:** logs only in stderr + `~/.saga/logs/*.log`; `agent_env()` has no `OTEL_*`; no spans.
- **After (unit-verifiable):** with `CORALOGIX_*` set, `build_log_handler` yields an OTLP handler, `agent_env()` carries the `OTEL_*` block with `task.id`, `StepRunner.run`/`send` emit spans (asserted via in-memory exporter). With `CORALOGIX_*` unset, everything is byte-identical to today.
- **End-to-end (needs live endpoint+key, cannot verify in this env):** Criterion 1 — a `saga.orchestrator.loop` dispatch log appears as a structured Coralogix doc with severity/service/task.id; Criterion 2 — a `saga.step` trace with `saga.claude_turn` child span; Criterion 3 — Claude Code telemetry correlated by `task.id` resource attribute. Surface these as a manual post-deploy check in the PR (same untestable-without-creds pattern as `GITHUB_APP_PRIVATE_KEY`).

## Artifacts
Before-state is code-confirmed (no OTEL wiring today); capture `just test` output for `test_agent_env.py` (green baseline) and, after implementation, the new telemetry/span test output into `.saga/artifacts/`.