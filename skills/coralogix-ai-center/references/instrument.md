# Phase 2 of 4 — Instrument

Choose the instrumentation, write the code that emits OpenTelemetry GenAI spans, and wire the export path. This phase uses no Coralogix credentials.

**Input** — the Assess handoff: maturity path (M1–M4), who-runs (skill-run / user-run), environments in scope, call paths to instrument, collector answer, naming convention, upstream flag.

**Output** — files changed; flag names; `cx.application.name` / `cx.subsystem.name` values; how to run the app; per-environment leftovers (deploy-config file + the exact lines that would enable them); and, for user-run, the filled run checklist from [run.md](run.md).

Design to the gates in [verify.md](verify.md) — read them before writing code. Its "Required attributes per span type" section is the single source for what each span must carry. AI Center reads trace data only from the S3 archive; the prerequisite is checked in [assess.md](assess.md).

## Principles

- **Laser focus on the AI interaction, in tiers.** MUST: inference spans with the required attributes, including `gen_ai.input.messages` and `gen_ai.output.messages` — an integration without message content does not pass. SHOULD: conversation id, user id, `execute_tool` spans. IF-APPLICABLE / full: request-handler root span, workflow and skill-step spans carrying the step identity, `invoke_agent` spans. Whole-service APM beyond the AI call paths is a separate offer, made after verification passes.
- **Registry attributes only.** Emit only `gen_ai.*` attributes present in the OTel GenAI semconv registry or the Coralogix inventory, because attributes from proposal PRs and vendor extensions do not exist for AI Center.
- **Trace-consistent attributes.** A value derived or corrected on one span (provider, conversation id, user) is inherited by every span type in the trace, so one conversation never reports two providers.
- **Pass config values explicitly and verify them against the installed library's parser** — [stacks.md](stacks.md) carries the variable and accepted values per library. The same value is permissive in one library and strictly rejected in another, and a strict parser can degrade to capturing nothing with only a log warning.
- **Loud failure over silent degradation.** A swallowing guard around telemetry code (bare except, warn-and-continue, an `is not None` skip) costs both the data and the alarm.
- **Attach cross-cutting attributes at a level every span passes through** — a span processor's on-start hook, not app-level decorators, which miss auto-instrumentor spans.
- **When an SDK ships its own telemetry backend, route prompts to Coralogix instead.** Clear the vendor upload, then confirm the switch you used stops uploading rather than span generation.
- **Kill switches default OFF in code and are turned on per environment in the deploy config.** Content capture always gets its own switch so prompts can be disabled without losing tracing, but it is ON in every environment in scope — turning it off anywhere is the user's explicit, recorded decision, never a default; add an export switch only when the app has no existing export control. Local dev stays off with a documented one-line opt-in, and every environment in scope must be switched ON — a flag off everywhere delivers nothing. Wire the switch ON in each in-scope environment's config file and cite file:line; inline command-line flags wire nothing.
- **Token semantics:** `gen_ai.usage.input_tokens` is the non-cached input count; never add cache tokens into it.

## Research ladder

Per call path, take the highest rung that resolves, and verify every rung with a live lookup rather than from memory.

1. **Coralogix.** The stack row in [stacks.md](stacks.md), then the compatibility matrix (`cx docs fetch "user-guides/ai/otel-integration/providers.md"`) and the copy-paste scripts in `user-guides/ai/otel-integration/code-examples.md` (Python, Java, .NET, Go).
2. **The open-source ecosystem.** Check sources in order: OpenLLMetry (https://github.com/traceloop/openllmetry); `open-telemetry/opentelemetry-python-contrib` `instrumentation-genai/`; `open-telemetry/opentelemetry-python-genai` `instrumentation/`; `open-telemetry/opentelemetry-js-contrib`; the semconv-genai conformance matrix (https://github.com/open-telemetry/semantic-conventions-genai → `reference/README.md`), which lists per span type which libraries have verified instrumentation; then a web search and a PyPI/npm search. Guessing package names is not a lookup. Vet each candidate against the code paths the app actually uses — which method it wraps, which conventions it emits, whether its pins are installable. **Prefer an emitter that produces the new shape** — `gen_ai.provider.name` with `gen_ai.input.messages` / `gen_ai.output.messages`. A legacy-only emitter is acceptable because AI Center parses both shapes: OpenLLMetry today emits `gen_ai.system` with `gen_ai.prompt.{i}.*` / `gen_ai.completion.{i}.*`, and its content switch is `TRACELOOP_TRACE_CONTENT=true`. `OTEL_SEMCONV_STABILITY_OPT_IN` does not flip a legacy emitter, so check the shape it actually writes; [stacks.md](stacks.md) is the lookup.
3. **Thin shim over a near-miss.** When a package wraps a sibling method, misses one hook, or needs a small adapter or subclass, a shim on top of it beats hand-writing the whole span model.
4. **The traffic choke point.** When the calls already flow through a gateway or proxy (an `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL` override, a router), the gateway's own telemetry support can emit GenAI spans for every client passing through it, including CLIs nothing can instrument in-process. Scope the emitters so each call path has exactly one. Before ruling a gateway out: fetch its telemetry doc live (LiteLLM: `cx docs fetch "integrations/ai-observability/ai-apps/litellm.md"`), check its scoping options (per-key/team metadata, per-route callbacks, resource attributes) and whether the app already tags its calls. "Shared with other traffic" is a scoping task, not a disqualifier; record the outcome either way.
5. **Manual spans.** Author the `gen_ai.*` spans yourself per the required-attributes table in [verify.md](verify.md), and record what was searched and ruled out.

A resolved rung is implemented in the same task. Ask the user only when two rungs are genuinely viable and the trade-off is theirs to make.

### Agent SDKs and CLI-driven agents

For the Claude Agent SDK, Claude Code, Codex/Copilot CLIs, and other sandboxed runners, the process sees a message stream, not the HTTP calls.

- One `chat` span per assistant message: each model response is one call; take model and usage from that message when the SDK exposes them, else from the turn result on the last call.
- The `invoke_agent` span is the parent and carries agent identity only (`gen_ai.agent.name`, `gen_ai.conversation.id`) — never the messages.
- Tool calls become `execute_tool` children (hooks or tool-result events) carrying `gen_ai.tool.call.id`.
- The provider comes from the response model / gateway route.
- If the stream cannot yield per-call spans, the gateway (rung 4) is the emitter — say so in the report; a single summary span per turn is not an option.

## Implementation

**Install the tracer provider globally** so app spans and every instrumentor join the same traces. A process-local provider produces orphaned GenAI subtrees with no request around them; if you scope it narrower on purpose, state why.

**Author the interaction tree to the tier you committed to.** Where the app has no tracing, create the request-handler root span, the spans inside tools around their backend calls, and the workflow or skill-step spans carrying the step identity. Where it has them, verify the GenAI spans nest under them.

**Set `gen_ai.provider.name` to the semconv well-known value for the provider actually called:**

| Provider | Value |
|---|---|
| OpenAI · Anthropic | `openai` · `anthropic` |
| AWS Bedrock · Azure | `aws.bedrock` · `azure.ai.openai` · `azure.ai.inference` |
| Google | `gcp.vertex_ai` · `gcp.gemini` · `gcp.gen_ai` |
| Mistral · Cohere · DeepSeek | `mistral_ai` · `cohere` · `deepseek` |
| Groq · Perplexity · xAI · IBM watsonx | `groq` · `perplexity` · `x_ai` · `ibm.watsonx.ai` |

**Behind a gateway, override `gen_ai.provider.name` per call** from the model actually routed: one static provider for every call means wrong breakdowns and wrong cost.

**Failed calls set span status `ERROR` and record the exception.** AI Center counts errors only from `otel.status_code == ERROR`, taking the message from `otel.status_description`, so an error caught and logged without touching span status is invisible.

**JSON-valued attributes are string-encoded JSON, never an object.** Every message carries `role`, and parts are typed `text`, `tool_call`, or `tool_call_response`.

**Pin a version ceiling when a shim touches private surfaces** (underscore attributes or methods), make a mismatch fail loudly instead of skipping silently, and let the wiring tests be the upgrade gate.

**Wire the tracer provider's shutdown into the app's teardown, ordered LAST.** Batch span processors buffer; without `force_flush()` / `shutdown()` on exit, every deploy, restart, and Ctrl-C drops the final batch — including the spans emitted while other resources close.

## Export path

- **M3 — collector present** → the app exports to the existing collector, and the Coralogix exporter is added to **its** config, wherever that config lives. One path only, no parallel direct export alongside it.
- **M1 / M2** → deploy a collector (recommended) or export directly via OTLP.

A collector handles auth, batching, and retry, keeping credentials out of application code. `private_key` is a Send-Your-Data key (`cxtp_`), collected in [run.md](run.md) — reference it as an env var here, never as a literal.

```yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: "0.0.0.0:4317"

exporters:
  coralogix:
    domain: "<region>.coralogix.com"        # e.g. eu2.coralogix.com
    private_key: "${CORALOGIX_PRIVATE_KEY}" # Send-Your-Data API key
    application_name: "my-genai-app"
    subsystem_name: "my-service"
    application_name_attributes:
      - "cx.application.name"
    subsystem_name_attributes:
      - "cx.subsystem.name"
    timeout: 30s

service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [coralogix]
```

**Alternative: direct OTLP export (no collector):**

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT="https://ingress.<region>.coralogix.com:443"
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer <send-your-data-api-key>"
export OTEL_RESOURCE_ATTRIBUTES="cx.application.name=my-app,cx.subsystem.name=my-subsystem"
```

**Application environment variables (collector path):**

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"
export OTEL_EXPORTER_OTLP_INSECURE="true"
export OTEL_SERVICE_NAME="my-ai-service"
export OTEL_RESOURCE_ATTRIBUTES="cx.application.name=my-app,cx.subsystem.name=my-subsystem"
export OTEL_SEMCONV_STABILITY_OPT_IN=gen_ai_latest_experimental
# Message-content capture: the variable AND the accepted value depend on the installed
# library version — see stacks.md. OTel contrib: OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT
# =true, or an enum in newer builds: NO_CONTENT (default) | SPAN_ONLY | EVENT_ONLY | SPAN_AND_EVENT.
# OpenLLMetry: TRACELOOP_TRACE_CONTENT=true.
export OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT=true
```

### Existing-pipeline checklist (M3)

| Check | Why |
|---|---|
| Head or tail sampling does not drop GenAI spans | Sampled-out spans under-count tokens and cost |
| `filter` / `transform` / `attributes` processors keep the `gen_ai.*` attributes | A drop or redaction rule silently removes message attributes |
| `OTEL_ATTRIBUTE_VALUE_LENGTH_LIMIT` and `OTEL_SPAN_ATTRIBUTE_VALUE_LENGTH_LIMIT` left unset | Truncated message JSON no longer parses |
| OTLP gRPC receiver `max_recv_msg_size_mib` and batch `send_batch_max_size` sized for message payloads | The receiver's default 4 MiB message limit rejects large prompt batches |
| `application_name_attributes: [cx.application.name]` and `subsystem_name_attributes: [cx.subsystem.name]` on the Coralogix exporter | Otherwise every app lands under the exporter's static names |

**Cover every environment the user asked for.** Find where env vars and config reach each one (k8s env defaults, Helm values, Terraform, `.env.*` files) and wire both enablement and export there. A collector that exists only in `docker-compose.yml` covers local dev alone, so deliver the production half too or use direct export and say why.

**Document every new variable in the repo's env example file** (`.env.example` or equivalent), with a warning on the content-capture flag that enabling it exports user prompts and model responses.

**Upstream-bound integrations stay vendor-neutral:** a generic OTLP endpoint and `OTEL_RESOURCE_ATTRIBUTES`, with Coralogix-specific values appearing only as documented examples.

**Unknown or self-hosted models need a pricing override** — register them with `cx ai-center model-pricing set` (JSON file, `--yes`), or their cost stays 0. The four `gen_ai.*_price` attributes are Coralogix enrichment and are never emitted by the app.

## Multi-agent systems

- **Type a subagent's own execution as an `invoke_agent` span, not a generic tool span.** The agent graph renders only from `invoke_agent` spans, and a bare tool span hides the subagent's internal structure.
- **Don't emit duplicate dispatch and execution nodes.** An `execute_tool` dispatch span plus a sibling `invoke_agent` span double-represents one event; keep the bare tool span only as a fallback when the subagent's execution is invisible.
- **Nest recursively.** The subagent's `invoke_agent` span sits under the span that orchestrates the dispatch, as a sibling of the inference span that requested it, with the subagent's own inference, tool, and nested agent spans beneath it.
- **Name subagents distinctly** via `gen_ai.agent.name`. Frameworks often default every subagent to the same role name, so derive a specific name from the subagent's actual task.

## Before handing off

A checklist the report echoes line by line:

1. Wiring tests added (file).
2. Tracer shutdown / `force_flush()` wired last in teardown (file:line).
3. Every new variable in `.env.example` with the content-capture warning.
4. Both switches ON per in-scope environment (file:line).
5. Test suite does not export telemetry.
6. Gates walked on paper — every gated attribute has an emitter, every model call has an inference span.

Symptom-driven debugging lives in [pitfalls.md](pitfalls.md).
