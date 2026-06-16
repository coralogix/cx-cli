# Claude Code Reference

> **PromQL syntax:** See `promql-guidelines.md` + `metrics-querying.md` for the query language reference.
> **Session text (DataPrime):** See `dataprime-reference.md` + `logs-querying.md`.

Claude Code reports data via two sources:
- **Metrics** (`claude_code_*`): cost, tokens, sessions, tools, code impact — use PromQL via `cx metrics`.
- **Session logs** (`ai_sessions_claude`): full conversation turn-by-turn messages — use DataPrime via `cx logs`.

---

## Metrics Data Model

Important labels: `user_email`, `model`, `session_id`, `type`, `decision`, `tool_name`, `repository_name`, `cx_application_name`, `cx_subsystem_name`.

| Metric | Description |
|---|---|
| `claude_code_cost_usage_USD_total` | Cost in USD |
| `claude_code_token_usage_tokens_total` | Token usage — split by `type`: `input`, `output`, `cacheRead`, `cacheCreation` |
| `claude_code_session_count_total` | Session count (no `model` label; use cost metric for model-aware session counts) |
| `claude_code_active_time_total_s_total` | Active time in seconds |
| `claude_code_lines_of_code_count_total` | Lines of code changed |
| `claude_code_commit_count_total` | Git commits |
| `claude_code_pull_request_count_total` | Pull requests |
| `claude_code_code_edit_tool_decision_total` | Edit tool decisions; `decision="accept"` for acceptance rate |
| `claude_code_session_repo_info` | Session-to-repository mapping |

---

## PromQL Queries

### Cost

```bash
# Total cost over the selected range
cx metrics query-range 'sum(increase(claude_code_cost_usage_USD_total{<filters>}[<range>]))' \
  --start now-7d
```

```promql
sum(increase(claude_code_cost_usage_USD_total{<filters>}[<range>]))
```

```promql
sum by (model) (increase(claude_code_cost_usage_USD_total{<filters>}[<range>]))
```

```promql
topk(<N>, sum by (user_email) (increase(claude_code_cost_usage_USD_total{<filters>}[<range>])))
```

```promql
sum by (session_id) (increase(claude_code_cost_usage_USD_total{<filters>}[<range>]))
```

```promql
sum by (session_id, model) (increase(claude_code_cost_usage_USD_total{<filters>}[<range>]))
```

### Tokens

```bash
# Total tokens over the last week, broken down by user
cx metrics query 'topk(10, sum by (user_email) (increase(claude_code_token_usage_tokens_total[7d])))'
```

```promql
sum(increase(claude_code_token_usage_tokens_total{<filters>}[<range>]))
```

```promql
sum(increase(claude_code_token_usage_tokens_total{type="input",<filters>}[<range>]))
```

```promql
sum by (user_email) (increase(claude_code_token_usage_tokens_total{<filters>}[<range>]))
```

### Cache Hit Rate

```promql
sum(increase(claude_code_token_usage_tokens_total{type="cacheRead",<filters>}[<range>]))
/
sum(increase(claude_code_token_usage_tokens_total{type=~"input|cacheRead|cacheCreation",<filters>}[<range>]))
* 100
```

### Sessions

```bash
# Unique sessions over the last week
cx metrics query 'count(count by (session_id) (increase(claude_code_token_usage_tokens_total[7d])))'
```

```promql
count by (user_email) (count by (user_email, session_id) (increase(claude_code_cost_usage_USD_total{<filters>}[<range>])))
```

```promql
count by (model) (count by (model, session_id) (increase(claude_code_cost_usage_USD_total{<filters>}[<range>])))
```

```promql
count(count by (session_id) (increase(claude_code_token_usage_tokens_total{<filters>}[<range>])))
```

### Session-to-Repository Mapping

```promql
max by (session_id, repository_name) (max_over_time(claude_code_session_repo_info{<app-subsystem-filters>}[<range>]))
```

### Code Impact

```promql
sum(increase(claude_code_code_edit_tool_decision_total{decision="accept",<filters>}[<range>]))
/
sum(increase(claude_code_code_edit_tool_decision_total{<filters>}[<range>]))
* 100
```

```promql
topk(<N>, sum by (tool_name) (increase(claude_code_code_edit_tool_decision_total{<filters>}[<range>])))
```

---

## Session Text (DataPrime)

Retrieve turn-by-turn conversation messages for a specific session. The `ai_sessions_claude` source uses two log shapes; use `firstNonNull` to normalize across both.

```bash
cx logs 'source ai_sessions_claude
| filter ($d.logRecord.attributes["session.id"] == "<session_id>" || $d.attributes["session_id"] == "<session_id>")
| filter ["claude_code.user_prompt", "claude_code.api_response_body"].arrayContains($d.logRecord.body) || ["claude_code.user_prompt", "claude_code.api_response_body"].arrayContains($d.body)
| create body_norm from firstNonNull($d.logRecord.body, $d.body)
| create resp_body from firstNonNull($d.logRecord.attributes["body"], $d.attributes["body"])
| create prompt_norm from firstNonNull($d.logRecord.attributes["prompt"], $d.attributes["prompt"])
| create ts_norm from firstNonNull($d.logRecord.timeUnixNano, $d.timeUnixNano)
| extract $d.resp_body into parsed using jsonobject()
| create role from case {
    $d.body_norm == "claude_code.user_prompt" -> "user",
    $d.body_norm == "claude_code.api_response_body" -> "assistant",
    _ -> "unknown"
  }
| orderby $d.ts_norm asc
| choose $d.role as role, $d.prompt_norm as prompt, $d.parsed.content as content_blocks, $d.parsed.text as flat_text, $d.ts_norm as ts' \
  --start now-7d --tier archive -o agents
```

```text
source ai_sessions_claude
| filter ($d.logRecord.attributes['session.id'] == '<session_id>' || $d.attributes['session_id'] == '<session_id>')
| filter ['claude_code.user_prompt', 'claude_code.api_response_body'].arrayContains($d.logRecord.body) || ['claude_code.user_prompt', 'claude_code.api_response_body'].arrayContains($d.body)
| create body_norm from firstNonNull($d.logRecord.body, $d.body)
| create resp_body from firstNonNull($d.logRecord.attributes['body'], $d.attributes['body'])
| create prompt_norm from firstNonNull($d.logRecord.attributes['prompt'], $d.attributes['prompt'])
| create ts_norm from firstNonNull($d.logRecord.timeUnixNano, $d.timeUnixNano)
| extract $d.resp_body into parsed using jsonobject()
| create role from case {
    $d.body_norm == 'claude_code.user_prompt' -> 'user',
    $d.body_norm == 'claude_code.api_response_body' -> 'assistant',
    _ -> 'unknown'
}
| orderby $d.ts_norm asc
| choose $d.role as role, $d.prompt_norm as prompt, $d.parsed.content as content_blocks, $d.parsed.text as flat_text, $d.ts_norm as ts
```
