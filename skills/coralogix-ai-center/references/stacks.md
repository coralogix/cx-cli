# Stacks — instrumentor lookup

This table is rung 1 of the research ladder in [instrument.md](instrument.md). Versions verified 2026-09-15 — confirm the current version and its content switch live before use.

## How to read

- Coralogix-verified = listed in the Coralogix compatibility matrix (`cx docs fetch "user-guides/ai/otel-integration/providers.md"`); other rows are community/vendor findings with (H)/(M)/(L) confidence in Notes.
- Semconv: **New** = `gen_ai.provider.name` + `gen_ai.input.messages`/`gen_ai.output.messages` (string JSON). **Legacy** = `gen_ai.system` + `gen_ai.prompt.{i}.*`/`gen_ai.completion.{i}.*`. **Vendor** = non-`gen_ai.*` attributes, not detected by AI Center.
- AI Center parses New and Legacy; prefer New when the stack has it.

## Table

| Stack | Lang | Instrumentor (version) | Semconv | Content switch | Opt-in | Notes |
|---|---|---|---|---|---|---|
| Anthropic | Py | `opentelemetry-instrumentation-anthropic` (0.62.3, OpenLLMetry) | Legacy | `TRACELOOP_TRACE_CONTENT` | On by default | Coralogix-verified; new-shape migration incomplete, traceloop/openllmetry#3515 (M) |
| AWS Strands | Py | `strands-agents` native | — | — | — | Coralogix-verified |
| Azure OpenAI | Py | `opentelemetry-instrumentation-openai-v2` (2.4b0, OTel contrib) | Legacy default / New opt-in | `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` | Off by default | Coralogix-verified; New needs `OTEL_SEMCONV_STABILITY_OPT_IN=gen_ai_latest_experimental`; deprecated upstream → `opentelemetry-instrumentation-genai-openai` (H) |
| AWS Bedrock | Py | `opentelemetry-instrumentation-bedrock` (≥0.60.0; 0.62.3 current, OpenLLMetry) | Legacy | `TRACELOOP_TRACE_CONTENT` | On by default | Coralogix-verified (M) |
| CrewAI | Py | `opentelemetry-instrumentation-crewai` (0.62.3, OpenLLMetry) | Legacy | `TRACELOOP_TRACE_CONTENT` | On by default | Coralogix-verified (M) |
| Google ADK | Py | `opentelemetry-instrumentation-google-genai` (1.1b1, OTel contrib) | New | `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` | Off by default | Coralogix-verified; covered via google-genai SDK path (M) |
| Google GenAI (Gemini) | Py | `opentelemetry-instrumentation-google-genai` (1.1b1, OTel contrib) | New | `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` | Off by default | Coralogix-verified; values unconfirmed — verify at use; provider `gcp.vertex_ai`/`gcp.gemini` (M) |
| LangChain | Py | `opentelemetry-instrumentation-langchain` (0.62.3, Traceloop pkg) | Legacy | `TRACELOOP_TRACE_CONTENT` | On by default | Coralogix-verified; OTel-style name but a Traceloop package (M) |
| LiteLLM | Py | `litellm` (≥1.86.0) native, `callbacks: ["otel"]` | New (opt-in var mandatory) | `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` | Off by default | Coralogix-verified; setup guide `integrations/ai-observability/ai-apps/litellm.md`; see Gateways row |
| LlamaIndex | Py | `opentelemetry-instrumentation-llamaindex` (0.62.3, OpenLLMetry) | Legacy | `TRACELOOP_TRACE_CONTENT` | On by default | Coralogix-verified (M) |
| Ollama | Py | OpenLLMetry | Legacy | `TRACELOOP_TRACE_CONTENT` | On by default | Coralogix-verified; version unconfirmed — verify at use (M) |
| OpenAI (completions) | Py | `opentelemetry-instrumentation-openai-v2` (2.4b0, OTel contrib) | Legacy default / New opt-in | `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` | Off by default | Coralogix-verified; same package as Azure OpenAI row (H) |
| OpenAI Agents | Py | `opentelemetry-instrumentation-openai-agents-v2` (OTel contrib) | New | `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` | Off by default | Coralogix-verified |
| Vertex AI | Py | `opentelemetry-instrumentation-google-genai` (via google-genai SDK) — or Traceloop `-vertexai` pkg (legacy) | New or Legacy | `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` / `TRACELOOP_TRACE_CONTENT` | Off / On by default | Coralogix-verified; which package actually runs is unconfirmed — verify at use |
| Mastra | Node | `@mastra/core` (1.67.0) native OTel bridge | New (v1.38) | unverified | — | Coralogix-verified (M) |
| OpenAI | Node | `@opentelemetry/instrumentation-openai` (0.20.0, OTel-JS-contrib) | unconfirmed | unconfirmed | unconfirmed | Not Coralogix-verified — verify at use (M) |
| Anthropic / OpenAI / LangChain | Node | `@traceloop/instrumentation-*` (0.27.0) + `@traceloop/node-server-sdk` | Legacy | `TRACELOOP_TRACE_CONTENT` | On by default | Not Coralogix-verified (M) |
| Google Generative AI | Node | `@traceloop/instrumentation-google-generativeai` (0.27.0) | Legacy | `TRACELOOP_TRACE_CONTENT` | On by default | Not Coralogix-verified (L) |
| Vercel AI SDK ≤v6 | Node | `ai` `experimental_telemetry` | Vendor (`ai.*`) | `recordInputs`/`recordOutputs` (per call) | Off by default | Not Coralogix-verified; AI Center does not detect these spans (H) |
| Vercel AI SDK v7+ | Node | `@ai-sdk/otel` (1.0.101) `OpenTelemetry` mode | New (`LegacyOpenTelemetry` mode = vendor `ai.*`) | unverified | unverified | Not Coralogix-verified (M) |
| Vertex AI (enterprise SDK) | Node | none found | — | — | — | Gap — use Gateways row or manual spans (M) |
| Spring AI | Java | `spring-ai-model` (1.0.0) native Micrometer→OTel | New (v1.37) | `ObservationRegistry` config | code config | Not Coralogix-verified; no OpenLLMetry for Java exists (M/H) |
| Other Java | Java | manual spans | New | — | — | Per `code-examples.md` Java example |
| Anthropic (anthropic-sdk-go) | Go | `opentelemetry-go-compile-instrumentation` (experimental, compile-time) | New + Legacy dual-emit | — | — | Not Coralogix-verified; no streaming support (M) |
| go-openai | Go | none found | — | — | — | Gap — manual spans per `code-examples.md` Go example (M) |
| Microsoft.Extensions.AI | .NET | `Microsoft.Extensions.AI` (10.10.0) `OpenTelemetryChatClient` native | New (v1.37) | `EnableSensitiveData=true` or `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` | Off by default | Not Coralogix-verified (H) |
| LiteLLM proxy/SDK | any | native `otel` callback | New (opt-in var mandatory) | `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT=SPAN_ONLY` | Off by default | Coralogix-verified; `OTEL_SEMCONV_STABILITY_OPT_IN=gen_ai_latest_experimental` mandatory; one emitter for any client via `OPENAI_BASE_URL`/`ANTHROPIC_BASE_URL`, incl. CLIs nothing else instruments |
| Claude Code | any | built-in telemetry | Vendor (`claude_code.*`) | `OTEL_LOG_USER_PROMPTS`/`OTEL_LOG_TOOL_CONTENT`/`OTEL_LOG_RAW_API_BODIES`, gated by `CLAUDE_CODE_ENABLE_TELEMETRY=1` + `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1` | Off by default | Not `gen_ai.*` — feeds Code Agents screen, not Application Catalog (H) |
| Claude Agent SDK | Py/TS | `opentelemetry-instrumentation-genai-claude-agent-sdk` (open-telemetry/opentelemetry-python-genai, verify release state) or community `opentelemetry-claude-agent-sdk` / `otel-instrumentation-claude-agent-sdk` (wrap `receive_response()` only — check the call path uses it) — else the Agent-SDK pattern in instrument.md or the gateway | New | `capture_content=True` | Off by default | Not Coralogix-verified; `invoke_agent`/`execute_tool` spans; some Claude Code versions ignored `ANTHROPIC_BASE_URL` when spawned by Agent SDK — verify on current version (M); SDK process routed to LiteLLM via `ANTHROPIC_BASE_URL` → gateway is a rung-4 candidate |

## Notes

- Vercel AI SDK ≤v6 spans are `ai.*` vendor shape — AI Center does not detect them; upgrade to v7 `@ai-sdk/otel` for `gen_ai.*`.
- Claude Code's built-in telemetry is separate from the Claude Agent SDK `gen_ai.*` path — see the last two table rows.
- Traceloop/OpenLLMetry packages (Python and Node) are legacy-shape only; that shape PASSes AI Center's gates (see [verify.md](verify.md)), but prefer New where a New-shape instrumentor exists for the same call.
- `opentelemetry-instrumentation-openai-v2` is deprecated upstream in favor of `opentelemetry-instrumentation-genai-openai` — re-check on next verify.
- LangChain over Vertex: set `gen_ai.provider.name` to `gcp.vertex_ai`, not `openai`/`anthropic`, even though the instrumentor is the LangChain package.
- A gateway should be the single emitter per call path — don't stack a client-side instrumentor and the gateway's own telemetry on the same call (see [instrument.md](instrument.md)).
- All `gen_ai.*` attributes are "Development" stability in the OTel GenAI semconv — expect changes between library versions; pin and re-verify on upgrade.
- Provider well-known values: `openai`, `anthropic`, `aws.bedrock`, `azure.ai.openai`, `azure.ai.inference`, `gcp.vertex_ai`, `gcp.gemini`, `gcp.gen_ai`, `mistral_ai`, `cohere`, `deepseek`, `groq`, `perplexity`, `x_ai`, `ibm.watsonx.ai`.
