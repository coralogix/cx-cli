# AI Center — GenAI span querying (DataPrime), schema & playbooks

This is the telemetry half of the AI Center skill: how to read GenAI interactions and
compute AI metrics from **spans**, using the cx CLI. Run every query with:

```bash
cx spans '<DataPrime query>' --start now-1d --end now
```

`cx spans` defaults to the last 1 hour when you don't pass `--start`; widen it (e.g.
`--start now-1d`) when the question needs more history and the user hasn't specified a window.
Use `-o json` for machine-readable output. For general DataPrime/spans syntax see the
`cx-telemetry-querying` skill and [dataprime-reference.md](dataprime-reference.md) /
[spans-querying.md](spans-querying.md). For **configuration** (inventory, evaluations,
policies, coverage, pricing) use the `cx ai-center` commands documented in the parent
`SKILL.md` — that data is not in spans.

> **Shell quoting:** DataPrime uses single quotes for string literals, so wrap the whole
> query in double quotes and escape the `$` sigils (`\$d`, `\$l`, `\$m`) so the shell doesn't
> expand them — or put the query in a file and pipe it in. Examples below show the raw
> DataPrime; quote as needed for your shell.

---

## Golden rule: read the interactions, not just the verdicts

Each interaction's **GenAI spans** carry the **conversation** plus model, tokens, cost, latency,
errors, and tool calls. Two conventions store the conversation — current
(`gen_ai.input.messages` / `gen_ai.output.messages`) and older indexed (`gen_ai.prompt.<n>` /
`gen_ai.completion.<n>`); see **Reading conversations (content questions)** for each one's exact
shape and how to read it. Evaluations, when configured, add **optional** verdict tags on the GenAI
span; guardrails are **optional** too and live on **separate** guardrail spans
(`otel.library.name == 'cx_guardrails.client'`). **Match the data to the
question:** for **content** questions (quality, hallucination, sentiment, frustration,
satisfaction, topics) read the conversation and reason about it; for everything else use the
relevant tags/aggregations. For plain dialogue read the **`user`**/**`assistant`** turns (via the **Reading conversations
(content questions)** queries below), but the **`system`**/**`tool`** turns matter too when you're
debugging an agent or sub-agent (the tool calls it made and the task it was given). The system
prompt lives in the `gen_ai.system_instructions` tag **or** the `role:"system"` turn: the first
`role:"system"` message inside `gen_ai.input.messages` (current) / `gen_ai.prompt.0.content` when
`gen_ai.prompt.0.role == "system"` (indexed). Check both places.

Read the interactions the question needs, ordered by recency — don't add a manual `limit`.
Report how many matched vs. how many you read (e.g. "312 matched; I read the 200 most
recent") and offer to narrow. Cite the `traceID` of any interaction you reference.

Use `cx ai-center applications list` to get the exact `applicationName`/`subsystemName` pairs.
The app filter in the queries below is **optional**: keep it to scope to one app, **drop it** for
org-wide questions, or **group by** `$l.applicationName, $l.subsystemName` to compare.

**To read the actual user+assistant transcript** — both conventions, system prompt and tool
traffic excluded — use the queries under **Reading conversations (content questions)** below.
Don't hand-select the raw `input.messages` blob for content questions: it carries the system
prompt and tool traffic and bloats context.

---

## How AI Center data is stored

**Granularity.** An **AI span** = one AI operation (an LLM call, embeddings, retrieval/agent/
tool/workflow step, or a Guardrails SDK invocation). An **interaction** = the multiple spans
under one `traceID` — one exchange plus the surrounding spans and operations. Counts (AI spans,
errors, guardrail actions) are **span-level**; the only trace-level rate is Issue Rate (Q9). For
a deduplicated interaction count use `distinct_count(traceID)` and say so.

**Find GenAI spans — the mandatory filter on every AI Center query.** `source spans` also
holds ordinary APM traffic; omitting this filter computes AI metrics over non-AI data and
returns wrong answers.
```dataprime
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
```
To also include guardrail spans (for AI-span counts and issue/guardrail rates),
add `|| tags['otel.library.name']:string == 'cx_guardrails.client'`. Guardrail spans are marked
by `tags['otel.library.name'] == 'cx_guardrails.client'`; from there, filter by whatever the
question needs — `tags['guardrails.triggered'] == 'true'` for **triggered** guardrails, `== 'false'`
for ones that **passed**, or the per-policy `gen_ai.{prompt|response}.guardrails.{policy}.triggered`
tags for a specific guardrail.

> An **AI application** is a GenAI `($l.applicationName, $l.subsystemName)` **pair** — the apps
> returned by `cx ai-center applications list`, not every `applicationName` in spans. **AI error
> rate** = `otel.status_code == 'ERROR'` on GenAI spans (not generic HTTP 5xx). When unsure
> which apps are AI apps, get them from `cx ai-center applications list`.

**Scoping to an application.** A name may live in either `applicationName` or `subsystemName`;
if a filter returns nothing, try the other field, or use `cx search-fields "<name>" --dataset spans`
to find which field holds it. Time = `$m.timestamp` (`roundTime($m.timestamp, <interval>ms)`
for series); duration = `$m.duration` (µs).

**End-user identity:**
```dataprime
| create user from firstNonNull(tags['enduser.id'], tags['user.id'], tags['gen_ai.request.user'], tags['traceloop.association.properties.user_id'], tags['langsmith.metadata.user_id'])
```

**Message formats.** `gen_ai.input.messages` / `gen_ai.output.messages` are JSON arrays of
`{role, parts:[{type, content}]}` (part `type` = `text`, `tool_call`, `tool_call_response`,
`reasoning`, `blob`/`file`/`uri`). (The older indexed convention stores the same
roles as flat per-message tags — `gen_ai.prompt.<n>.{role,content}` /
`gen_ai.completion.<n>.{role,content}`, no `parts` array — see Reading conversations below.)

### Reading conversations (content questions)

**Two conventions carry the conversation — handle both** (pick by which tags exist: use the
**Current** query when `gen_ai.input.messages` is present, the **Indexed** query when
`gen_ai.prompt.0.role` is present):
- **Current:** `gen_ai.input.messages` / `gen_ai.output.messages` — a **single JSON blob** per side
  holding **all** turns as `{role, parts:[{type, content}]}` objects.
- **Older indexed:** **one tag per message**, numbered from 0 — `gen_ai.prompt.<n>.role` /
  `gen_ai.prompt.<n>.content` for each **input** message and `gen_ai.completion.<n>.role` /
  `gen_ai.completion.<n>.content` for the **output** message(s). `gen_ai.prompt.0` is the first
  message (usually the system prompt); the conversation is spread across `prompt.0, prompt.1, …`
  and `completion.0, …`. An assistant turn that calls tools carries
  `gen_ai.prompt.<n>.tool_calls.*` (function name/arguments/id) and **no** `.content`; a tool
  **result** comes back as `role:"tool"` with its output in `.content`.

**Current convention — general conversation read.** Parse the messages JSON with `jsonobject()`,
keep `user`/`assistant` roles (drops `system` + `tool`) and `type:"text"` parts (drops `tool_call`
/ `tool_call_response`), and collect one transcript per span. Scope by adding
`| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'` after `source spans`; a
span's `input.messages` accumulates the prior turns, so the fullest span of a conversation holds the
whole history:
```dataprime
source spans
| filter $d.tags['gen_ai.input.messages'] != null || $d.tags['gen_ai.output.messages'] != null
| create input_json from concat('{"messages":', firstNonNull($d.tags['gen_ai.input.messages'], '[]'), '}')
| create output_json from concat('{"messages":', firstNonNull($d.tags['gen_ai.output.messages'], '[]'), '}')
| extract $d.input_json into input_parsed using jsonobject()
| extract $d.output_json into output_parsed using jsonobject()
| create all_messages from arrayConcat($d.input_parsed.messages, $d.output_parsed.messages)
| explode $d.all_messages into $d.message original preserve
| filter ['user','assistant'].arrayContains($d.message.role)
| explode $d.message.parts into $d.part original preserve
| filter $d.part.type == 'text' && trim(firstNonNull($d.part.content, '')) != ''
| create line from concat($d.message.role, ': ', $d.part.content)
| groupby $d.traceID as trace_id, $d.spanID as span_id aggregate collect($d.line) as lines
| create conversation_text from arrayJoin($d.lines, '\n')
| choose trace_id, span_id, conversation_text
```
This keeps `type:"text"` parts only. Some SDKs embed model reasoning (`<thinking>…</thinking>`) or
serialize a tool call into a text part (e.g. `ResponseFunctionToolCall(…)`) — those are inside the
content, so they can appear in the transcript; treat them as noise when judging.

Aggregate instead of read (e.g. top user questions): keep only `$d.message.role == 'user'` before
the parts explode, then `| groupby $d.part.content aggregate count() as times | orderby times desc`.

**Indexed convention — general conversation read.** The messages are separate numbered tags
(`gen_ai.prompt.<n>.*`), so there's no JSON string to parse and DataPrime can't enumerate the
numbered keys directly. Instead serialize the whole span with `$d:string` and `multi_regexp` every
`role`/`content` pair at once (any message count, no hardcoded range); group by index to re-pair
role with content, then keep `user`/`assistant` with non-empty content (drops `system`, `role:"tool"`
results, and tool-call turns which have no `.content`):
```dataprime
source spans
| filter $d.tags['gen_ai.prompt.0.role'] != null
| create doc_json from $d:string
| extract doc_json into matches using multi_regexp(e=/"gen_ai\.(prompt|completion)\.(\d+)\.(role|content)":"((?:[^"\\]|\\.)*)"/)
| explode matches into m original preserve
| extract m into p using regexp(e=/"gen_ai\.(?<grp>prompt|completion)\.(?<idx>\d+)\.(?<kind>role|content)":"(?<value>(?:[^"\\]|\\.)*)"/)
| groupby $d.traceID as trace_id, $d.spanID as span_id, p.grp:string as grp, p.idx:number as idx
    aggregate any_value(if(p.kind == 'role', p.value:string, null)) as role,
              any_value(if(p.kind == 'content', p.value:string, null)) as content
| filter (role == 'user' || role == 'assistant') && content != null && trim(content) != ''
| create line from concat(if(grp == 'prompt', '0', '1'), ':', padLeft(idx:string, 5, '0'), ':::', role, ': ', content)
| groupby trace_id, span_id aggregate arrayJoin(arraySort(collect(line)), '\n') as conversation_text
| redact conversation_text matching /[01]:\d{5}:::/ to ''
| choose trace_id, span_id, conversation_text
```
The `grp`/`idx` sort prefix orders prompt-before-completion and ascending index (`collect` doesn't
preserve order); `redact` strips it. Same content-level caveat as the current convention
(`<thinking>…` / tool-call-as-text can appear). Scope with
`| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'`.

**Span attributes (GenAI spans).** Span kind — `gen_ai.operation.name`: `chat` (also
`generate_content`, `text_completion`), `embeddings`, `retrieval` (RAG), `create_agent`,
`invoke_agent`, `invoke_workflow`, `execute_tool`.

| Attribute | Meaning |
| --- | --- |
| `gen_ai.provider.name` (or deprecated `gen_ai.system`) | Provider |
| `otel.status_code` (`ERROR`) / `error.type` | Failure status / error class |
| `otel.library.name` | `cx_guardrails.client` marks a guardrail span |
| `gen_ai.agent.{id,name,description,version}` / `gen_ai.workflow.name` | Agent / workflow identity |
| `gen_ai.request.model` / `gen_ai.response.model` / `gen_ai.response.id` | Requested / actual model; completion id |
| `gen_ai.conversation.id` | Conversation/session/thread id |
| `enduser.id` / `user.id` (+ fallbacks above) | End user |
| `gen_ai.usage.input_tokens` / `gen_ai.usage.output_tokens` / cache token fields | Token usage |
| `gen_ai.prompt_price` / `gen_ai.response_price` / `gen_ai.read_cache_price` / `gen_ai.write_cache_price` | Cost (USD) |
| `gen_ai.response.finish_reasons` | Stop reason (`~ 'length'` = truncated; `~ 'tool_'` = tool call — values seen: `tool_call`/`tool_calls`/`tool_use`) |
| `gen_ai.input.messages` / `gen_ai.output.messages` (current) or `gen_ai.prompt.<n>` / `gen_ai.completion.<n>` (indexed) | Conversation transcript — read via the **Reading conversations (content questions)** queries, don't grep raw |
| `gen_ai.tool.{name,call.arguments,call.result}` / `gen_ai.tool.definitions` | Tools executed / advertised |
| `gen_ai.{target}.evaluations.{type}.{score,label}`, `…evaluations.custom.{0..9}.{…}`, `guardrails.triggered` | Eval/guardrail results (see below) |

**Cost (USD):** total = `firstNonNull(gen_ai.prompt_price,0)+firstNonNull(gen_ai.response_price,0)`;
cache: `gen_ai.read_cache_price`/`gen_ai.write_cache_price`. Cache token accounting differs by
provider: OpenAI-style `cache_read` ⊂ `input_tokens`; Anthropic-style input/cache_read/
cache_creation are disjoint. Custom per-model pricing is team-wide, new data only — read it via
`cx ai-center model-pricing get` (price tags already reflect it).

---

## Evaluations, Guardrails & Policies

A **policy** is a configurable check. The same policy can act as an **evaluation** (post-hoc
scoring/flagging) and/or a **guardrail** (real-time block). UI evaluation categories:
Hallucinations, Security, Toxicity, Topics, User experience, Compliance, plus Custom (typed
Quality or Security).

**Evaluations** run on spans and write results back as tags. The target is **`prompt` or
`response`** (the backend supports only these two — ignore the unused `conversation` enum value):
- Built-in: `gen_ai.{prompt|response}.evaluations.{eval_type}.{score|label|version|details}`.
  **`label == 'p1'` marks a flagged issue.**
- Custom: `gen_ai.{target}.evaluations.custom.{0..9}.{name,target,category,triggered,score,label}`
  (`triggered` is the string `"true"`/`"false"`).
- Eval types — Hallucinations: `hallucination_context_adherence|context_relevance|completeness|correctness|task_adherence`, `sql_hallucination`;
  Security: `prompt_injection`, `pii`, `sql_read_only|sql_load|sql_restricted_tables|sql_allowed_tables`;
  Toxicity: `toxicity`, `sexism`; Topics: `restricted_topics`, `allowed_topics`, `competition`;
  User experience: `language_mismatch`.
- **Issue class:** Security = `prompt_injection`, `pii`, `sql_*`; everything else = Quality.

**Guardrails** act in real time via the `cx-guardrails` SDK and **block** on violation. Each
invocation is a span (`otel.library.name == 'cx_guardrails.client'`) with
`tags['guardrails.triggered'] == 'true'` when it triggered; `$l.operationName` starts with
`guardrails.prompt`/`guardrails.response`. Prebuilt: Prompt Injection, PII, Toxicity — plus
**custom** guardrails.

**Configuration is not telemetry.** Which policies exist, which are enabled per app, the
inventory, guarded status (`guardrailsIntegrated`), coverage, and the team's **custom model
pricing** (`cx ai-center model-pricing get`) come from the backend — use the `cx ai-center`
commands, not span queries.

---

## DataPrime query library (Q1–Q15, runnable)

Q1–Q15 are the queries behind the AI Center UI widgets — here to **show the agent how each
metric is computed, not as mandatory copy-paste**. If the user asks something the UI already
answers, use the matching Q as-is; if it's close, take the relevant fields and adjust; if it's
new, compose your own using these as reference. Keep the GenAI filter, set the time range, and
scope to one app with `| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'`
(drop for org-wide).

**Q1 — Key insights (batched)** — Models Used, Time to Response, Token Usage, Estimated Cost,
Errors, Guardrail Actions, AI Spans, Unique Users:
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter ((tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))) || tags['otel.library.name']:string == 'cx_guardrails.client'
| create cost from firstNonNull(tags['gen_ai.prompt_price']:number,0) + firstNonNull(tags['gen_ai.response_price']:number,0)
| create tokens from firstNonNull(tags['gen_ai.usage.input_tokens']:number,0) + firstNonNull(tags['gen_ai.usage.output_tokens']:number,0)
| create user from firstNonNull(tags['enduser.id'], tags['user.id'], tags['gen_ai.request.user'], tags['traceloop.association.properties.user_id'], tags['langsmith.metadata.user_id'])
| aggregate count() as ai_spans, avg(duration) as ttr_avg, sum(tokens) as token_usage,
            sum(cost) as estimated_cost, count_if(tags['otel.status_code']:string == 'ERROR') as errors,
            count_if(tags['guardrails.triggered']:string.toLowerCase() == 'true') as guardrail_actions,
            distinct_count(user) as unique_users,
            collect(firstNonNull(tags['gen_ai.request.model']:string, ''), distinct=true) as models_used
```
> Q1 is a **batched** query computing many metrics at once. When the user asks for **one**
> metric, run a minimal query with only that aggregation (and the `create` lines it needs). TTR only → `… | aggregate avg(duration) as ttr_avg`
> (or `percentile(0.95, duration)`; selector 0.5/0.75/0.9/0.95/0.99). Issue Rate = Q9.

**Q2 — Response time (avg + percentiles):**
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
| aggregate avg(duration) as avg, percentile(0.75, duration) as p75, percentile(0.90, duration) as p90, percentile(0.95, duration) as p95, percentile(0.99, duration) as p99
```
For a response-time **trend over time**, add `| groupby roundTime($m.timestamp, <interval>ms) as Time` before the aggregate and `| orderby Time` after.

**Q3 — Latency by model:** as Q2 but `groupby firstNonNull(tags['gen_ai.request.model']:string,'unknown') as model`, `orderby avg desc`.

**Q4 — Top slowest apps/spans:** groupby `$l.applicationName, $l.subsystemName` (apps) or `$l.operationName` (spans), `aggregate avg(duration) as avg | orderby avg desc | limit 5`.

**Q5 — Cost & tokens (totals):**
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
| create input_tokens from firstNonNull(tags['gen_ai.usage.input_tokens']:number,0)
| create output_tokens from firstNonNull(tags['gen_ai.usage.output_tokens']:number,0)
| create cache_read from firstNonNull(tags['gen_ai.usage.cache_read.input_tokens']:number, tags['gen_ai.usage.cache_read_input_tokens']:number, 0)
| create cache_creation from firstNonNull(tags['gen_ai.usage.cache_creation.input_tokens']:number, tags['gen_ai.usage.cache_creation_input_tokens']:number, 0)
| create cost from firstNonNull(tags['gen_ai.prompt_price']:number,0) + firstNonNull(tags['gen_ai.response_price']:number,0)
| aggregate sum(cost) as cost, sum(input_tokens) as inputTokens, sum(output_tokens) as outputTokens, sum(cache_read) as cacheReadTokens, sum(cache_creation) as cacheCreationTokens
```
Cache Hit Rate = `sum(cacheReadTokens)/sum(inputTokens)`. For a cost/tokens **trend over time**, add `| groupby roundTime($m.timestamp, <interval>ms) as Time` before the aggregate and `| orderby Time` after.

**Q6 — Cost by model:** as Q5 but `groupby firstNonNull(tags['gen_ai.request.model']:string,'unknown') as model | orderby cost desc`.

**Q7 — Most expensive apps / high-spending users:** as Q5 but `groupby $l.applicationName,$l.subsystemName` (apps) or the `user` expr, `aggregate sum(cost) as cost, sum(input_tokens+output_tokens) as tokens | orderby cost desc | limit 5`.

**Q8 — Errors (count) / top errored:**
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
| aggregate count() as total, count_if(tags['otel.status_code']:string == 'ERROR') as errors
```
For an errors **trend over time**, add `| groupby roundTime($m.timestamp, <interval>ms) as Time` before the aggregate and `| orderby Time` after. Top errored apps/spans: `groupby $l.applicationName,$l.subsystemName` (or `$l.operationName`) `aggregate count_if(tags['otel.status_code']:string=='ERROR') as errors | orderby errors desc | limit 5`.

**Q9 — Issue rate (trace-level).** An issue on a target = any built-in eval `label == 'p1'`
OR any custom eval `custom.{0..9}` with `triggered == 'true'` OR a guardrail that triggered on
that target. Repeat the `{eval}` term per enabled eval type and the `custom.{n}` term per
configured index (for AI-SPM use only security types):
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter ((tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))) || tags['otel.library.name']:string == 'cx_guardrails.client'
| create prompt_issue from (tags['gen_ai.prompt.evaluations.{eval}.label']:string == 'p1' || tags['gen_ai.prompt.evaluations.custom.0.triggered']:string.toLowerCase() == 'true' || ($l.operationName.startsWith('guardrails.prompt') && tags['guardrails.triggered']:string.toLowerCase() == 'true'))
| create response_issue from (tags['gen_ai.response.evaluations.{eval}.label']:string == 'p1' || tags['gen_ai.response.evaluations.custom.0.triggered']:string.toLowerCase() == 'true' || ($l.operationName.startsWith('guardrails.response') && tags['guardrails.triggered']:string.toLowerCase() == 'true'))
| groupby traceID aggregate max(if(prompt_issue,1,0)) as p, max(if(response_issue,1,0)) as r
| aggregate count() as traces, sum(p) as prompt_issue_traces, sum(r) as response_issue_traces
| create prompt_issue_rate from prompt_issue_traces*100.0/traces
| create response_issue_rate from response_issue_traces*100.0/traces
```
Issue Distribution / Top Apps With Issues = the same detection grouped by eval **category**
(Security = `prompt_injection`/`pii`/`sql_*`; else Quality) or by `$l.applicationName`.

**Q10 — Tool calls:**
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
| create is_tool from (tags['gen_ai.operation.name']:string == 'execute_tool' || tags['gen_ai.response.finish_reasons']:string ~ 'tool_')
| filter is_tool
| extract tags['gen_ai.output.messages']:string into msg_tool using regexp(e=/"type"\s*:\s*"tool_call".*?"name"\s*:\s*"(?<name>[^"]+)"/)
| create tool from firstNonNull(tags['gen_ai.tool.name']:string, msg_tool.name, 'unknown')
| groupby tool aggregate count() as uses | orderby uses desc
```
Calls-per-interaction: `groupby traceID aggregate count_if(is_tool) as n | groupby n aggregate count() as cnt`.
Tool usage %: `aggregate distinct_count(traceID) as total, distinct_count_if(is_tool, traceID) as with_tools`.

**Q11 — Read interactions / AI Explorer:** use the **Reading conversations (content questions)**
queries (current-convention `jsonobject`, indexed-convention `$d:string` + `multi_regexp`) to get
the clean user+assistant transcript; add tokens/cost/user/duration tags alongside if needed.

**Q12 — Session walkthrough:** `filter traceID == '<traceID>'` then read ordered turns.

**Q13 — Who's using / sessions & user insights:**
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
| create user from firstNonNull(tags['enduser.id'], tags['user.id'], tags['gen_ai.request.user'], tags['traceloop.association.properties.user_id'], tags['langsmith.metadata.user_id'])
| filter user != null
| groupby user aggregate count() as interactions, distinct_count(tags['gen_ai.conversation.id']) as sessions,
            sum(firstNonNull(tags['gen_ai.prompt_price']:number,0)+firstNonNull(tags['gen_ai.response_price']:number,0)) as cost
| orderby interactions desc | limit 5
```
High-Activity = orderby interactions; High-Spend = orderby cost; Risky = add a security-eval
`count_if(...label=='p1')` and orderby that.

**Q14 — Agentic workflow walkthrough:**
```dataprime
source spans
| filter traceID:string == '<traceID>'
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
| choose $m.timestamp as ts, tags['gen_ai.operation.name']:string as step,
         tags['gen_ai.agent.name']:string as agent, tags['gen_ai.workflow.name']:string as workflow,
         tags['gen_ai.tool.name']:string as tool, tags['gen_ai.tool.call.arguments']:string as tool_args,
         tags['gen_ai.tool.call.result']:string as tool_result, tags['gen_ai.request.model']:string as model,
         tags['gen_ai.input.messages']:string as input_messages, tags['gen_ai.output.messages']:string as output_messages
| orderby ts asc | limit 200
```
This is a raw per-step dump (tools included) for debugging an agent run. For the clean
user+assistant transcript — either convention, system prompt/tools excluded — use the
**Reading conversations (content questions)** queries instead.

**Q15 — Evaluation score distribution / trend** (score is a 0–1 float, separate from `p1`):
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter tags['gen_ai.response.evaluations.{eval}.score']:number != null
| create bucket from round(tags['gen_ai.response.evaluations.{eval}.score']:number, 1)
| groupby bucket aggregate count() as n | orderby bucket
```
Trend: `groupby roundTime($m.timestamp, <interval>ms) as Time aggregate avg(tags['gen_ai.response.evaluations.{eval}.score']:number) as avg_score`. Swap `response`→`prompt` for the other target.

---

## Phase 1 — interaction intelligence (read the content)

**Method for sentiment / satisfaction / frustration:**
1. Scope to recent interactions (order by recency; no manual `limit`). State matched-vs-read.
2. **Read the conversations — don't keyword-search for emotion.** Use the **Reading conversations
   (content questions)** queries (current or indexed convention) to get clean user+assistant
   transcripts, group by session (`gen_ai.conversation.id`, fall back to `traceID`), and read each
   session's turns in order.
3. **Judge from the USER's side.** Read user turns: satisfied (got their answer, "thanks",
   no re-asks) vs not (re-asks/rephrasing, rising turns, negative wording, excessive `?`/`!`/
   CAPS, "that's not what I asked", abandonment). Response quality / `finish_reason` / evals are
   secondary — never the headline. Don't pivot a satisfaction question into "the app is broken".

**Answer shape:** summarize the split, then back it with a few **verbatim user-message quotes +
their `traceID`** (satisfied and dissatisfied) — an example is the user's own words, not a
description of the agent's reply. Don't dump raw ID lists.

- **Topic analysis** ("what do users ask about?") is an LLM-judgment task — read recent user
  turns and cluster the asks yourself; report themes + how many you read. Don't bucket with
  keyword filters.
- **Counting a specific term** ("how often do users mention RUM?") is narrower — decide if they
  want a count, list, or examples. Match **user turns** at the **word level** (a bare `~ 'rum'`
  also hits *forum/premium*) — extract them first with the **Reading conversations (content questions)** queries — and
  **never grep the whole `gen_ai.input.messages` blob (or the indexed `prompt.<n>` tags)**: they
  carry the system prompt + tool definitions, so they match nearly everything. Report counts as a
  floor.
- **Quality / hallucination:** optionally pre-filter `…evaluations.{name}.label=='p1'`, but
  confirm by reading the messages.
- **Root cause:** `otel.status_code=='ERROR'` (+ `error.type`), then read messages + errored
  child spans; watch truncation, tool failures, retrieval misses. **Pull the whole trace
  (`filter $d.traceID == '<id>'`), not just the GenAI spans** — the cause often lives in a
  sibling **non-GenAI** span (a DB call, HTTP request, retrieval step).
- **Agent behaving off-task / deviating from its instructions:** read the **system prompt**
  (`gen_ai.system_instructions` and/or the `role:'system'` turn) alongside the user/assistant
  turns, and judge whether the responses stayed within the task the system prompt defines.
  Cite the `traceID`s where it drifted.

---

## Examples (question → approach)
> When a question names an app/agent, **run `cx ai-center applications list` first** to resolve
> the exact `applicationName`/`subsystemName` pair before scoping a query.

- *"Which of my agents / LLM apps do I have?"* → `cx ai-center applications list`.
- *"Average time to response for Financial Advisor last 24h?"* → list apps to get the exact
  pair, then **Q1** (`ttr_avg`) scoped to it, `--start now-1d` (or **Q2** for the trend;
  `percentile(0.95, duration)` for P95).
- *"Token spend by model this week?"* → **Q6**, `--start now-7d`.
- *"Which apps have the highest error rate?"* → **Q8** top variant, no app scope.
- *"Which region / agent is the most active?"* → GenAI filter, no app scope, `groupby
  $l.applicationName, $l.subsystemName aggregate count() as spans | orderby spans desc`.
- *"Which agent deviates from its intended task?"* → read each agent's **system prompt** +
  user/assistant turns and judge drift (see the *Agent behaving off-task* bullet in Phase 1);
  cite traceIDs.
- *"Compare agent X now vs a week ago"* / *"compare agent X to agent Y"* → run the relevant Q
  twice with different `--start`/`--end` windows, or grouped by `$l.applicationName,$l.subsystemName`,
  and diff the results (e.g. cost, error rate, latency, or the system prompt via a trace from each).
- *"Are users frustrated in app X?"* → Phase 1 frustration playbook (**Q11** to read the
  conversations; quote user turns + traceIDs).
- *"Which apps lack guardrails?"* → `cx ai-center applications list -o json | jq '[.[]|select(.guardrailsIntegrated==false)]'` (config).
- *"What policies are configured for app X?"* → `cx ai-center evaluations list --application <app> --subsystem <sub>`.

## Troubleshooting
- **No rows for an app** — the name may be in `subsystemName` not `applicationName`; try the
  other field or `cx search-fields "<name>" --dataset spans`; widen the time range.
- **Empty `gen_ai.*.messages`** — the app isn't capturing content (opt-in); say so.
- **No eval/guardrail tags** — those policies aren't enabled; still answer content questions by
  reading messages.
- **Guarded status / configured policies / inventory / coverage** — configuration; use the
  `cx ai-center` commands, not spans.
- **AI Security Posture Score** — backend-computed; no CLI command or span query returns it.
  Say so rather than trying to derive it.
- **AI App Discovery** (GitHub repo scan) — not reachable from the CLI (the scan service only
  accepts user-session auth). Explain it exists; don't attempt to fetch it.
