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

Each interaction's spans carry the **conversation** (`gen_ai.input.messages` = the full chat
history sent to the model — system/user/assistant/tool turns — and `gen_ai.output.messages` =
the model's response) plus model, tokens, cost, latency, errors, tool calls, and pre-computed
eval/guardrail verdict tags. **Match the data to the question:** for **content** questions
(quality, hallucination, sentiment, frustration, satisfaction, topics) read the input/output
messages and reason about them; for everything else use the relevant tags/aggregations. When
reading a conversation use only the **user** and **assistant** turns; skip `system`/`tool`.
Pull the system prompt only for system-prompt questions (`gen_ai.system_instructions` and/or
the `role:'system'` message, usually index 0 — check both).

Read the interactions the question needs, ordered by recency — don't add a manual `limit`.
Report how many matched vs. how many you read (e.g. "312 matched; I read the 200 most
recent") and offer to narrow. Cite the `traceID` of any interaction you reference.

Example — reading a conversation (drop the app filter for a global question):
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter ['chat','generate_content','text_completion'].arrayContains(tags['gen_ai.operation.name']:string) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
| choose $m.timestamp as ts, $d.traceID as trace_id,
         $l.applicationName as application, $l.subsystemName as subsystem,
         tags['gen_ai.conversation.id']:string as conversation,
         tags['gen_ai.request.model']:string as model,
         tags['gen_ai.input.messages']:string as input_messages,
         tags['gen_ai.output.messages']:string as output_messages
| orderby ts desc
```

---

## How AI Center data is stored

**Granularity.** An **AI span** = one AI operation (an LLM call, embeddings, retrieval/agent/
tool/workflow step, or a Guardrails SDK invocation). An **interaction** = the full trace
(`traceID`) of one exchange. Count widgets are **span-level** (matching the UI); the only
trace-level rate is Issue Rate (Q9). For a deduplicated interaction count use
`distinct_count(traceID)` and say so.

**Find GenAI spans — the mandatory filter on every AI Center query.** `source spans` also
holds ordinary APM traffic; omitting this filter computes AI metrics over non-AI data and
returns wrong answers.
```dataprime
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
```
For views that also include guardrail spans (AI Spans count, Issue-rate / Guardrail widgets),
add `|| tags['otel.library.name']:string == 'cx_guardrails.client'`. Guardrail spans have
`tags['otel.library.name'] == 'cx_guardrails.client'` (and `tags['guardrails.triggered'] == 'true'` when fired).

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
`reasoning`, `blob`/`file`/`uri`). Roles: `user` = the end user's turn; `assistant` = the
model's turn (may carry `tool_call` parts); `tool` = a tool's output fed back (`role:'tool'`,
not `user`); `system` = the system prompt (usually index 0). To read what the user asked, take
`text` parts of `role:'user'` messages.

> **Never use the deprecated indexed convention** (`gen_ai.prompt.<n>.content` /
> `gen_ai.completion.<n>.content`) — current data isn't written that way; querying them returns
> null/stale results. Always use `gen_ai.input.messages` / `gen_ai.output.messages` directly,
> even if `cx search-fields` surfaces the old keys.

**Extracting conversation text** (cheaper than parsing the whole blob). First-turn user ask +
assistant reply — note the content capture `(?:[^"\\]|\\.)*` spans escaped quotes so messages
aren't truncated:
```dataprime
| extract tags['gen_ai.input.messages']:string into u using regexp(e=/"role"\s*:\s*"user"[\s\S]*?"type"\s*:\s*"text"[\s\S]*?"content"\s*:\s*"(?<text>(?:[^"\\]|\\.)*)"/)
| extract tags['gen_ai.output.messages']:string into a using regexp(e=/"role"\s*:\s*"assistant"[\s\S]*?"content"\s*:\s*"(?<text>(?:[^"\\]|\\.)*)"/)
| choose $d.traceID as trace_id, u.text as user_text, a.text as assistant_text
```
Every turn (multi-turn): concat input history with the model's reply, match with
`multi_regexp`, `explode`, re-extract:
```dataprime
| create convo from concat(tags['gen_ai.input.messages']:string, tags['gen_ai.output.messages']:string)
| extract convo into turns using multi_regexp(e=/"role"\s*:\s*"(?:user|assistant)"[\s\S]*?"content"\s*:\s*"(?:[^"\\]|\\.)*"/)
| explode turns into turn original preserve
| extract turn into m using regexp(e=/"role"\s*:\s*"(?<role>\w+)"[\s\S]*?"content"\s*:\s*"(?<text>(?:[^"\\]|\\.)*)"/)
| choose $d.traceID as trace_id, m.role, m.text
```

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
| `gen_ai.response.finish_reasons` | Stop reason (`~ 'length'` = truncated, `~ 'tool_call'` = tool use) |
| `gen_ai.input.messages` / `gen_ai.output.messages` | Conversation transcript (read these for content) |
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
scoring/flagging) and/or a **guardrail** (real-time block). UI categories: Hallucinations,
Security, Toxicity, Topics, User experience, Compliance, plus Custom (typed Quality or Security).

**Evaluations** run on spans and write results back as tags:
- Built-in: `gen_ai.{prompt|response|conversation}.evaluations.{eval_type}.{score|label|version|details}`.
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
`tags['guardrails.triggered'] == 'true'` when it fired; `$l.operationName` starts with
`guardrails.prompt`/`guardrails.response`. Prebuilt: Prompt Injection, PII, Toxicity.

**Configuration is not telemetry.** Which policies exist, which are enabled per app, the
inventory, guarded status (`guardrailsIntegrated`), and coverage come from the backend — use
the `cx ai-center` commands, not span queries.

---

## DataPrime query library (Q1–Q15, runnable)

Keep the GenAI filter, set the time range, and scope to one app with
`| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'` (drop for org-wide).

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
> Q1 is the UI's **batched** query. When the user asks for **one** metric, run a minimal query
> with only that aggregation (and the `create` lines it needs). TTR only → `… | aggregate avg(duration) as ttr_avg`
> (or `percentile(0.95, duration)`; selector 0.5/0.75/0.9/0.95/0.99). Issue Rate = Q9.

**Q2 — Response time over time:**
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
| groupby roundTime($m.timestamp, <interval>ms) as Time
  aggregate avg(duration) as avg, percentile(0.75, duration) as p75, percentile(0.90, duration) as p90, percentile(0.95, duration) as p95, percentile(0.99, duration) as p99
| orderby Time
```

**Q3 — Latency by model:** as Q2 but `groupby firstNonNull(tags['gen_ai.request.model']:string,'unknown') as model` (no time bucket), `orderby avg desc`.

**Q4 — Top slowest apps/spans:** groupby `$l.applicationName, $l.subsystemName` (apps) or `$l.operationName` (spans), `aggregate avg(duration) as avg | orderby avg desc | limit 5`.

**Q5 — Cost & tokens over time:**
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
| create input_tokens from firstNonNull(tags['gen_ai.usage.input_tokens']:number,0)
| create output_tokens from firstNonNull(tags['gen_ai.usage.output_tokens']:number,0)
| create cache_read from firstNonNull(tags['gen_ai.usage.cache_read.input_tokens']:number, tags['gen_ai.usage.cache_read_input_tokens']:number, 0)
| create cache_creation from firstNonNull(tags['gen_ai.usage.cache_creation.input_tokens']:number, tags['gen_ai.usage.cache_creation_input_tokens']:number, 0)
| create cost from firstNonNull(tags['gen_ai.prompt_price']:number,0) + firstNonNull(tags['gen_ai.response_price']:number,0)
| groupby roundTime($m.timestamp, <interval>ms) as Time
  aggregate sum(cost) as cost, sum(input_tokens) as inputTokens, sum(output_tokens) as outputTokens, sum(cache_read) as cacheReadTokens, sum(cache_creation) as cacheCreationTokens
| orderby Time
```
Cache Hit Rate = `sum(cacheReadTokens)/sum(inputTokens)`. Drop `roundTime` for total KPIs.

**Q6 — Cost by model:** as Q5 without time bucket, `groupby firstNonNull(tags['gen_ai.request.model']:string,'unknown') as model | orderby cost desc`.

**Q7 — Most expensive apps / high-spending users:** as Q5 without time bucket, `groupby $l.applicationName,$l.subsystemName` (apps) or the `user` expr, `aggregate sum(cost) as cost, sum(input_tokens+output_tokens) as tokens | orderby cost desc | limit 5`.

**Q8 — Errors over time / top errored:**
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter (tags['gen_ai.system']:string != null || tags['gen_ai.provider.name']:string != null || tags['gen_ai.operation.name']:string != null) && !(['cursor-agent','codex_cli_rs','codex-app-server','github-copilot','gemini-cli'].arrayContains($l.serviceName))
| groupby roundTime($m.timestamp, <interval>ms) as Time
  aggregate count() as total, count_if(tags['otel.status_code']:string == 'ERROR') as errors
| orderby Time
```
Top errored: replace groupby with `$l.applicationName,$l.subsystemName` (or `$l.operationName`), `aggregate count_if(tags['otel.status_code']:string=='ERROR') as errors | orderby errors desc | limit 5`.

**Q9 — Issue rate (trace-level).** An issue on a target = any built-in eval `label == 'p1'`
OR any custom eval `custom.{0..9}` with `triggered == 'true'` OR a guardrail that fired on
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
| create is_tool from (tags['gen_ai.operation.name']:string == 'execute_tool' || tags['gen_ai.response.finish_reasons']:string ~ 'tool_call')
| filter is_tool
| extract tags['gen_ai.output.messages']:string into msg_tool using regexp(e=/"type"\s*:\s*"tool_call".*?"name"\s*:\s*"(?<name>[^"]+)"/)
| create tool from firstNonNull(tags['gen_ai.tool.name']:string, msg_tool.name, 'unknown')
| groupby tool aggregate count() as uses | orderby uses desc
```
Calls-per-interaction: `groupby traceID aggregate count_if(is_tool) as n | groupby n aggregate count() as cnt`.
Tool usage %: `aggregate distinct_count(traceID) as total, distinct_count_if(is_tool, traceID) as with_tools`.

**Q11 — Read interactions / AI Explorer:** the "read a conversation" query at the top (select
`gen_ai.input.messages` / `gen_ai.output.messages`, tokens, cost, user, duration).

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

**Q15 — Evaluation score distribution / trend** (score is a 0–1 float, separate from `p1`):
```dataprime
source spans
| filter $l.applicationName == '<APP>' && $l.subsystemName == '<SUB>'
| filter tags['gen_ai.response.evaluations.{eval}.score']:number != null
| create bucket from round(tags['gen_ai.response.evaluations.{eval}.score']:number, 1)
| groupby bucket aggregate count() as n | orderby bucket
```
Trend: `groupby roundTime($m.timestamp, <interval>ms) as Time aggregate avg(tags['gen_ai.response.evaluations.{eval}.score']:number) as avg_score`. Swap `response`→`prompt`/`conversation` for other targets.

---

## Phase 1 — interaction intelligence (read the content)

**Method for sentiment / satisfaction / frustration:**
1. Scope to recent interactions (order by recency; no manual `limit`). State matched-vs-read.
2. **Read the conversations — don't keyword-search for emotion.** Group by session
   (`gen_ai.conversation.id`, fall back to `traceID`) and read each session's turns in order.
3. **Judge from the USER's side.** Read user turns: satisfied (got their answer, "thanks",
   no re-asks) vs not (re-asks/rephrasing, rising turns, negative wording, excessive `?`/`!`/
   CAPS, "that's not what I asked", abandonment). Response quality / `finish_reason` / evals are
   secondary — never the headline. Don't pivot a satisfaction question into "the app is broken".

**Answer shape:** lead with a plain summary (roughly the split), then show evidence — a few
satisfied sessions (quote the user turn + `traceID`) and a few dissatisfied ones. An example is a
**verbatim quote of the user's own message** + traceID, not a description of the agent's reply.
Never paste raw lists of trace/span IDs.

- **Topic analysis** ("what do users ask about?") is an LLM-judgment task — read recent user
  turns and cluster the asks yourself; report themes + how many you read. Don't bucket with
  keyword filters.
- **Counting a specific term** ("how often do users mention RUM?") is narrower — decide if they
  want a count, list, or examples. Match **user turns** at the **word level** (a bare `~ 'rum'`
  also hits *forum/premium*), and **never grep the whole `gen_ai.input.messages` blob** (it
  contains the system prompt + tool definitions, so it matches nearly everything). Report counts
  as a floor.
- **Quality / hallucination:** optionally pre-filter `…evaluations.{name}.label=='p1'`, but
  confirm by reading the messages.
- **Root cause:** `otel.status_code=='ERROR'` (+ `error.type`), then read messages + errored
  child spans; watch truncation, tool failures, retrieval misses.

---

## Widget → query reference (which Qn each AI Center UI widget uses)

**Overview (global) & Application Drilldown (per app):** Models Used / Time to Response /
Token Usage / Estimated Cost / Errors / Guardrail Actions / AI Spans / Unique Users → **Q1**;
Response Time Trends → **Q2**; Latency by Model → **Q3**; Top Slowest → **Q4**; Cost & Tokens
over time / Cache Hit Rate → **Q5**; Cost by Model → **Q6**; Most Expensive Apps / High-Spend
Users → **Q7**; Errors over time / Top Errored → **Q8**; Issue Rate / Prompt & Response Issues
/ Issue Distribution / Top Apps With Issues → **Q9**; Tool Calls → **Q10**.

**Application Catalog:** header KPIs = Q1/Q9 without app scope; the grid = `cx ai-center
applications list` (gives `guardrailsIntegrated`) merged with per-app Q1 metrics grouped by
`$l.applicationName,$l.subsystemName`; Guarded % = calc from the list.

**AI Explorer** (the "read the messages" grid): **Q11**. Columns: Timestamp, Input, Output,
Tokens, Cost, User, Security issues, Quality issues, Duration.

**Policy Catalog / Policy Configuration** (config, not spans): `cx ai-center evaluations list` /
`cx ai-center custom-evaluations list` / `cx ai-center count`; per-app =
`cx ai-center evaluations list --application <app> --subsystem <sub>`. To change state use
`cx ai-center evaluations update` / `create` / `delete`, or `add-policy` / `remove-policy`.

**AI-SPM:** Total AI Applications = `cx ai-center applications list`; Total Security Violations
= security `p1` count (Q9 restricted to security types); User Insights = **Q13**. The **AI
Security Posture Score** is backend-computed — no query/command returns it; describe it and
point the user to the AI-SPM page. **AI App Discovery** (GitHub scan) is not available via the
CLI (the scan service only accepts user-session auth) — explain what it shows and point to the
AI-SPM page.

---

## Examples (question → approach)
- *"Average time to response for Financial Advisor last 24h?"* → **Q1** (`ttr_avg`) with
  `$l.subsystemName == 'Financial Advisor'`, `--start now-1d` (or **Q2** for the trend;
  `percentile(0.95, duration)` for P95).
- *"Token spend by model this week?"* → **Q6**, `--start now-7d`.
- *"Which apps have the highest error rate?"* → **Q8** top variant, no app scope.
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
