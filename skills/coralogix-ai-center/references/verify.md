# Phase 4 of 4 — Verify

Read the spans back from Coralogix and hard-validate them against the gates — the integration's only definition of done.

**Phase input (run handoff):** probe marker, entry point used, time window, `cx.application.name`/`cx.subsystem.name`, expected span types, who ran the app.
**Phase output:** the gates table, PASS/FAIL with evidence per gate; on any MUST FAIL, the gap list that goes back to [instrument.md](instrument.md).

## 1. Verification mode

| Mode | Prerequisite | The skill | The user | Verdict label |
|---|---|---|---|---|
| **skill-run verification** | a query key (`cxup_`) or configured profile, proved with one trivial `cx spans` call | runs every gate query itself, records the output | nothing | stated plainly, evidence attached |
| **user-run verification** | the user can run `cx` or open AI Center; the skill needs no key | hands over each gate query **with its expected values**, then reads the pasted output and rules on it | runs the queries, pastes the output back | **"user-reported"** on every gate |

A Send-Your-Data key (`cxtp_`) is ingest-only and cannot read data back; one sitting in `CX_API_KEY`/`CX_TOKEN` also shadows profile creds, so every query fails on scope (strip it with `env -u CX_API_KEY -u CX_TOKEN cx ...`). Missing query access selects user-run verification — it never blocks the phase.

## 2. Required attributes per span type

The single source for what must be on a span; [instrument.md](instrument.md) designs to this table.

| Span type (`gen_ai.operation.name`) | Required (MUST) | Recommended (SHOULD) | Notes |
|---|---|---|---|
| **Every GenAI span** | detection: `gen_ai.provider.name` (legacy fallback `gen_ai.system`) **or** `gen_ai.input.messages`; `gen_ai.operation.name` ∈ `chat`, `text_completion`, `generate_content`, `embeddings`, `invoke_agent`, `create_agent`, `agent_handoff`, `execute_tool`, `invoke_workflow`, `retrieval`; `cx.application.name` + `cx.subsystem.name` | `gen_ai.conversation.id`; `otel.status_description` next to an error status | Span typing reads the attribute only — the span name `{operation} {model}` is a semconv convention, informational. The UI also parses the legacy shape: an instrumentor that emits only `gen_ai.system` (OpenLLMetry/Traceloop) still passes detection. Every model call has an inference span; a trace whose GenAI spans are all wrapper types (`invoke_agent`…) FAILS G1 |
| **Inference** — `chat`, `text_completion`, `generate_content`, `embeddings` | `gen_ai.request.model`, `gen_ai.response.model`, `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, `gen_ai.input.messages` + `gen_ai.output.messages` — or, from a legacy-only instrumentor, the indexed `gen_ai.prompt.{i}.role` / `gen_ai.prompt.{i}.content` and `gen_ai.completion.{i}.role` / `gen_ai.completion.{i}.content`; `gen_ai.response.finish_reasons`; plus `gen_ai.usage.cache_read.input_tokens` and `gen_ai.usage.cache_creation.input_tokens` when the provider reports them | identity — `gen_ai.request.user` \| `enduser.id` \| `user.id`; `gen_ai.conversation.id`; `gen_ai.system_instructions`; tool definitions via `gen_ai.tool.definitions` (legacy `gen_ai.request.tools`) | There is no `cache_write` attribute. `cache_read_input_tokens`, `cached_tokens`, `cache_creation_input_tokens` are normalized on ingest. `gen_ai.usage.total_tokens` is informational. `gen_ai.prompt_price`, `gen_ai.response_price`, `gen_ai.read_cache_price`, `gen_ai.write_cache_price` are Coralogix enrichment, not app-emitted. OpenLLMetry/Traceloop (Python and Node) still emits the legacy shape in practice — prefer a new-shape emitter where the stack has one ([stacks.md](stacks.md)). AI Center renders messages only from inference spans; wrapper operations are parsed as metadata-only, their `gen_ai.*.messages` are dropped |
| **`execute_tool`** | `gen_ai.tool.name`, `gen_ai.tool.call.id`, `gen_ai.tool.call.arguments`, `gen_ai.tool.call.result` | the backend calls the tool makes, as child spans | `gen_ai.tool.call.id` has to be set explicitly; many instrumentors leave it empty |
| **`invoke_agent`** (and `create_agent`) | `gen_ai.agent.name` | `gen_ai.conversation.id`; inference and tool spans nested underneath | The agent graph renders **only** from `invoke_agent` spans, with `gen_ai.agent.name` as the node label |
| **`agent_handoff`** | `gen_ai.handoff.from_agent`, `gen_ai.handoff.to_agent` | `gen_ai.conversation.id` | The edges of the agent graph |

**Shape rules**

- `gen_ai.input.messages` / `gen_ai.output.messages` are **string-encoded JSON arrays** of `{role, parts: [{type: text | tool_call | tool_call_response, ...}]}`, with a `role` on every message. A language-native `repr`/`toString` blob stuffed into a `content` field passes a non-empty check and is corrupt for AI Center. Where the chosen instrumentor emits only the legacy indexed prompt/completion attributes, those are the accepted equivalent and are read the same way.
- The input array MUST contain at least one `role: "user"` entry carrying the actual prompt — assistant/system-only input is a FAIL. `gen_ai.response.finish_reasons` is likewise a JSON array encoded as a string.
- `gen_ai.provider.name` MUST be the semconv well-known value of the provider that actually served the call (`openai`, `anthropic`, `aws.bedrock`, `azure.ai.openai`, `gcp.vertex_ai`, `gcp.gemini`, …), resolved per call behind a gateway — never one static value for every call; a legacy-only emitter carries that same value on `gen_ai.system`.
- A failed call MUST carry `otel.status_code == ERROR` (with `otel.status_description`): AI Center counts errors from the span status only, never from message content.

## 3. Read the spans back

| Tier | Use | Latency |
|---|---|---|
| frequent | arrival checks only — did anything land, how many, which span names | seconds |
| **archive** | **every gate** — AI Center reads trace data only, and only from the S3 archive | minutes up to ~30 min; poll with a retry loop before concluding data is missing |

If a tag filter returns nothing on the frequent tier, filter by application/subsystem (`$l.applicationName` / `$l.subsystemName`) instead. **All gate evidence comes from `--tier archive`.**

```bash
cx spans "filter tags['gen_ai.provider.name']:string != null
  | select $m.traceID, $m.spanID, name,
           tags['gen_ai.operation.name']:string,
           tags['gen_ai.request.model']:string,
           tags['gen_ai.usage.input_tokens']:string,
           tags['gen_ai.usage.output_tokens']:string
  | limit 10" --tier archive --start now-30m --profile <profile>
```

Query patterns, field paths, and tips: [cli.md](cli.md).

**Prove the trace came from this run before auditing it.** Filter on the probe marker from the run handoff, or on a per-process stamp — `service.instance.id` is a fresh UUID per `Resource.create()`. Content inside the payloads proves nothing: shared datasets carry other processes' output, and a stale dev server from another checkout happily produces traces that would otherwise be audited as this run's.

**Prove the run itself is admissible.** The trace must come from the entry point recorded in the handoff — a real application run, crossing the process boundaries the change touches. A dev shortcut, a direct service invocation, or a synthetic call exercises none of the propagation the gates exist to check; if that is all that exists, go back to [run.md](run.md) for a real run rather than auditing it.

**Fetch the attribute inventory fresh — never audit from memory:**

```bash
cx docs fetch "user-guides/ai/otel-integration/span-attributes.md"
```

## 4. Gates

Record PASS/FAIL **with evidence** — the query used and the values observed. **One gates table per instrumented unit** (language, template, service): a unit without its own table is not verified, whatever the others show. N/A is admissible only when the application has no such construct at all (no agents, no tools, no failing call to exercise); a gate that is empty because of the integration's own design — a single summary span, no tool spans, a wrapper carrying the messages — is FAIL, never N/A.

| # | Tier | Gate | PASS criteria | Evidence to record |
|---|---|---|---|---|
| G1 | MUST | Detection + required attributes | every span on the AI path is detected as GenAI and carries the §2 Required column for its `gen_ai.operation.name`. A legacy-only instrumentor satisfies detection with `gen_ai.system`. And values are correct — `gen_ai.provider.name` is the provider that actually served the call, `gen_ai.request/response.model` the model served; a static provider behind a gateway FAILS | the §3 query output, one row per span type |
| G2 | MUST | Messages present and well-shaped | `gen_ai.input.messages` / `gen_ai.output.messages` non-empty on every inference span and valid per the §2 shape rules, the input holding a `role: "user"` entry with the probe prompt. Legacy indexed `gen_ai.prompt.{i}` / `gen_ai.completion.{i}` attributes PASS on the same terms — a `gen_ai.prompt.{i}.role` of `user` carrying the prompt. Content present only on a wrapper span = FAIL; zero inference spans = FAIL, not a vacuous PASS | the probe prompt text, quoted from inside the input array |
| G3 | MUST | Cost enrichment | `gen_ai.prompt_price` / `gen_ai.response_price` present on the inference spans — proof the tenant archives and AI Center parsed the spans, not merely stored them | the price values on one inference span |
| G4 | MUST | Catalog registration | the application appears in `cx ai-center applications list` | the application ID and the "View in Coralogix" URL the command prints |
| G5 | SHOULD | Request context | `gen_ai.conversation.id` and a user identity on the GenAI spans, for every identity the app actually has | the ids observed, or the statement that the app has no such identity |
| G6 | SHOULD | Within-trace consistency | one provider, one conversation id, one user across ALL span types of a trace — workflow, agent, inference, tool | the per-span values of one full trace |
| G7 | SHOULD | Interaction tree | GenAI spans nest under the request/handler root; tool spans wrap their backend calls; `invoke_agent` spans present when the app has agents | the span tree of one `$m.traceID` |
| G8 | SHOULD | Errors visible | a failed call carries `otel.status_code == ERROR` plus `otel.status_description` | the failed span with its status |
| G9 | SHOULD | Sensitive data | PII/confidential data masked or excluded from the captured content | what was inspected, what is masked |
| G10 | IF-APPLICABLE | Reference diff | against a known-good trace (production, the app's prior instrumentation, another instrumented service on the tenant): span types, hierarchy, and per-type attributes match; unexplained GenAI losses are gaps, not noise | the two traces side by side |
| G11 | IF-APPLICABLE | Model pricing | an unknown or self-hosted model has an override applied with `cx ai-center model-pricing set`; without it cost stays 0 | `cx ai-center model-pricing get` output |
| G12 | IF-APPLICABLE | No double counting | with more than one emitter on a call path, each call yields exactly one inference span and one set of token counts | span count per call on one trace |

## 5. Verdict

**Every MUST passes → done.** SHOULD failures are reported as recommendations, unless the user scoped that concern in during Assess — then they block like a MUST. The final message MUST end with:

**Report skeleton (copy verbatim, fill every line; exempt from any length budget):**

1. Gates table — all 12 rows, Tier | Result | Evidence (N/A only with the reason)
2. Emitter ladder — rung chosen; each rung ruled out + the lookup that ruled it out
3. Docs fetched — the `cx docs fetch` paths run in this session
4. Where the data lives — the "View in Coralogix" URL and application id from `cx ai-center applications list`, plus the path
5. Entry point used — command + label (customer path / dev shortcut) + probe marker + time window
6. Environments switched on — per environment: file:line of export switch and content switch
7. Scope statement — what was deliberately not done and why
8. Test-traffic inventory — apps/subsystems written to, probe ids, interaction counts, leftover processes (none)
9. Hygiene checklist — tests (file), shutdown (file:line), `.env.example`, suite-does-not-export
10. Offers — runbook / smoke script / gate re-assert script (ask before adding files)

**Any MUST fails → loop.** Emit the gap list (gate, evidence, suspected cause) → [instrument.md](instrument.md) to fix → re-run per the who-runs decision in [run.md](run.md) → re-verify, until every MUST passes. A gap caused by an upstream-library defect that cannot be fixed here becomes an issue-ready follow-up, and its gate stays FAIL unless the user accepts it as a known exception.
