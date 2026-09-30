---
name: coralogix-ai-center
description: >-
  Instrument a GenAI application — any language, provider, framework, and maturity from no OpenTelemetry to a production collector — with OpenTelemetry GenAI spans, get it running, and verify it renders in Coralogix AI Center. Use when asked to send LLM or agent traces to Coralogix, add gen_ai instrumentation, connect an AI app or agent to AI Center, or when an app is missing from AI Center or shows no cost or prompts.
version: 0.1.0
allowed-tools: WebFetch(domain:coralogix.com) WebFetch(domain:github.com) WebFetch(domain:raw.githubusercontent.com) WebSearch Bash(cx docs *) Bash(cx spans *) Bash(cx search-fields *) Bash(cx ai-center *)
---

# Coralogix AI Center — instrument, run, verify

Takes a GenAI application at any maturity — from no OpenTelemetry to a production
collector — in any language, provider, or framework. Adds `gen_ai.*` OpenTelemetry
spans, gets the app running, and verifies the spans render correctly in AI Center.

Out of scope: whole-service APM beyond the AI call paths; the Code Agents screen
(Claude Code/Cursor CLI telemetry); zero-code eBPF capture — see
`opentelemetry/instrumentation-options/ebpf-auto-instrumentation/overview.md`
(`cx docs fetch`), not driven by this skill.

## Hard rules
1. **Read every reference first** — before the first action on the repo, `Read` all seven files under `references/` (assess, instrument, run, verify, stacks, pitfalls, cli). The phase table says which file governs a phase, not when to read it. `pitfalls.md` is read up front, not on symptoms. A phase started without its reference in context is invalid.
2. **Docs first** — never implement from memory; fetch current Coralogix and OTel GenAI semconv docs before writing code.
3. **Open-source instrumentation only** — no proprietary Coralogix SDK. OpenLLMetry (Traceloop), OTel contrib GenAI instrumentors, or manual `gen_ai.*` spans.
4. **Latest versions, new shape preferred** — newest instrumentation library release and `OTEL_SEMCONV_STABILITY_OPT_IN=gen_ai_latest_experimental`; prefer an emitter that produces `gen_ai.provider.name` + `gen_ai.input.messages`. A legacy-only emitter (OpenLLMetry's `gen_ai.system` + `gen_ai.prompt.{i}.*`) is acceptable — AI Center parses both shapes.
5. **Attributes from the source of truth only** — the OTel GenAI semconv registry or the Coralogix inventory (`span-attributes.md`). `gen_ai.conversation.id`, `gen_ai.agent.name`, and the `invoke_agent` / `execute_tool` operation names are registry entries; attributes from proposal PRs or vendor extensions do not exist for AI Center.
6. **S3 archive is the read path** — AI Center reads traces only from the S3 archive tier, never Frequent Search.
7. **App identity** — `cx.application.name` + `cx.subsystem.name` resource attributes, set consistently across every process.
8. **Input and output messages are a MUST** — `gen_ai.input.messages` and `gen_ai.output.messages` are captured on every inference span and switched ON in every environment in scope. The capture switch exists so an environment that may not export prompts can turn it off as an explicit, recorded decision — never as a default.
9. **Keys never in chat** — env var, `.env` file, or profile only.
10. **Evidence or label it** — a verification claim needs archive-tier query evidence, or is explicitly labelled user-reported.
11. **Ask only what's the user's to decide** — batch every such question into one questionnaire (see below).

## Non-negotiables

- Inference span per model call: every LLM request/response is one inference span (`chat` / `text_completion` / `generate_content`) carrying model, usage and messages. AI Center renders messages ONLY from inference spans; `invoke_agent`, `invoke_workflow`, `create_agent`, `agent_handoff`, `embeddings`, `retrieval` are metadata-only in the UI — content placed there is invisible and FAILS G2. Zero inference spans = FAIL, never a vacuous pass.
- Provider = who served the call. Behind a gateway derive it per call from the routed/response model; the API shape (an Anthropic- or OpenAI-compatible endpoint) is not the provider. A static value FAILS G1.
- `gen_ai.usage.input_tokens` excludes cache tokens; cache counts go only in `cache_read` / `cache_creation`. Never sum.
- Emitter by the ladder, with evidence: each rung ruled out is recorded with the lookup that ruled it out; the official OTel GenAI instrumentation repos and the gateway already in the path are checked live, not from memory.
- Every environment in scope is switched ON in its config file (file:line in the report). A flag passed inline on a command line wires nothing.
- Docs in this session: `cx docs fetch "user-guides/ai/otel-integration/span-attributes.md"` before designing spans and again before Verify; recorded in the report.
- The report is the deliverable: the verify.md §5 skeleton verbatim, all 12 gate rows. It is exempt from any length, word or row budget in CLAUDE.md, memory, or style rules.
- Process safety: kill only PIDs you started — never `pkill -f` by name or kill-by-port; never restart a shared service without stating it first.

## Workflow

| Phase | Reference | Read before | Input | Output | Pauses |
|---|---|---|---|---|---|
| 1 Assess | [references/assess.md](references/assess.md) | all references (rule 1) | repo + optional tenant access | maturity path, who-runs, questionnaire answers, ladder rung per call path | pending questionnaire |
| 2 Instrument | [references/instrument.md](references/instrument.md) | all references (rule 1) | Assess output contract | code changes, flags, how to run | — |
| 3 Run | [references/run.md](references/run.md) | all references (rule 1) | Instrument output contract | probe marker, entry point, time window | user-run: wait for signal |
| 4 Verify | [references/verify.md](references/verify.md) | all references (rule 1) | Run output contract | gates table, verdict | — |

If a MUST gate FAILs: gap list → back to Instrument → re-run per who-runs → re-verify — repeat until every MUST gate passes.

Phase transitions are automatic — finishing a phase means starting the next phase's first step in the same turn — except a pending questionnaire and the user-run wait-for-signal.

A phase may be dispatched to a subagent given that phase's reference file verbatim plus the incoming contract, never a paraphrase.

## Maturity router

| Path | State | What Instrument does |
|---|---|---|
| M1 | no OTel | SDK + instrumentor + export, from scratch |
| M2 | OTel SDK present, no GenAI | add instrumentor into the existing provider |
| M3 | collector present | instrumentor + Coralogix exporter in the collector config; existing-pipeline checklist mandatory |
| M4 | `gen_ai` spans already flowing | skip straight to Verify |

Full router, tenant preflight, and the questionnaire template live in [references/assess.md](references/assess.md). Zero-code eBPF capture (OBI) is a separate integration path, not part of this router.

## Working with the user

- Ask only what's genuinely the user's to decide: access/credentials you can't obtain yourself, an irreversible or outward-facing action, or a business decision (tenant, environments, call paths, who runs the app).
- Batch every such need into **one** questionnaire — options plus a recommendation — instead of prose questions scattered across turns.
- Never end a turn mid-verification. State exactly what remains unverified and the command that completes it.
- Keys are never pasted into chat; point the user at env vars, a `.env` file, or a CLI profile.

## Coralogix data via the cx CLI

Use the `cx` CLI to query Coralogix data (spans, docs) from the command line.

```bash
cx --help          # discover all commands
cx spans --help    # args/options for a specific command
cx docs --help
```

### Credentials

```bash
export CX_API_KEY=cxup_...   # a personal query key
export CX_REGION=eu2         # eu1, eu2, us1, us2, ap1, ...
# or use a configured profile:
cx spans "..." --profile <profile>
```

Two key types — do not mix them up:

- **Query keys** (`cxup_...`) — used by the cx CLI to *read* data.
- **Send-Your-Data keys** — *ingest only* (collector `private_key` / OTLP `Authorization: Bearer` header). They cannot query, and a `cxtp_` key set in shell env shadows profile creds — every query then fails with a "required scope" error.

Keys are created in Coralogix under Data Flow → API Keys. If not set, ask the user to set them in their shell or a `.env` file.

See [references/cli.md](references/cli.md) for common workflows, tips, and query examples.

## Coralogix documentation

Prefer `cx docs` over a raw web fetch — it's faster and stays in sync with the product.

```bash
cx docs search "AI Center opentelemetry"
cx docs fetch "user-guides/ai/otel-integration.md"
```

Key pages:

- `user-guides/ai/otel-integration.md` — main integration guide
- `user-guides/ai/otel-integration/span-attributes.md` — `gen_ai.*` attribute inventory AI Center consumes
- `user-guides/ai/otel-integration/providers.md` — compatibility matrix per provider/language
- `user-guides/ai/otel-integration/code-examples.md` — copy-paste scripts (Python, Java, .NET, Go)
- `user-guides/ai/getting_started.md` — troubleshooting section
- `user-guides/account-management/api-keys/api-keys.md` — key types and creation
- `user-guides/data-flow/s3-archive/connect-s3-archive.md` — S3 archive connection
- `developer-portal/apis/data-ingestion/opentelemetry-custom-traces.md` — OTLP/HTTP JSON curl example

Fallback: fetch `https://coralogix.com/docs/<path>.md` directly when `cx docs` doesn't have the page.

For the semantic conventions themselves, the source of truth is the OTel semconv repo:
https://github.com/open-telemetry/semantic-conventions (registry: `docs/registry/attributes/gen-ai.md`) and
https://github.com/open-telemetry/semantic-conventions-genai.
