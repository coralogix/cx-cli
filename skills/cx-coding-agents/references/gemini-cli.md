# Gemini CLI Reference

> **PromQL syntax:** See `promql-guidelines.md` + `metrics-querying.md` for the query language reference.

Gemini CLI reports all data via **PromQL metrics** under the `gemini_cli_*` family.

---

## Metrics Data Model

Important labels: `user_email`, `model`, `type`, `programming_language`, `function_name`, `decision`, `cx_application_name`, `cx_subsystem_name`.

| Metric | Description |
|---|---|
| `gemini_cli_session_count_total` | Session count (no `model` label) |
| `gemini_cli_token_usage_total` | Token usage — split by `type`: `input`, `output`, `cache` |
| `gemini_cli_api_request_count_total` | API requests by model |
| `gemini_cli_file_operation_count_total` | File operations by `programming_language` |
| `gemini_cli_tool_call_count_total` | Tool calls by `function_name` and `decision` |

> **Note:** `gemini_cli_session_count_total` has no `model` label. If a model filter is active, approximate Gemini sessions by distributing session totals according to API-request share by model — call this out in the answer.

---

## PromQL Queries

### Sessions

```bash
# Total sessions over the last week
cx metrics query-range 'sum(increase(gemini_cli_session_count_total{<filters>}[1w]))' \
  --start now-7d
```

```promql
sum(increase(gemini_cli_session_count_total{<filters>}[<range>]))
```

### Active Users

```bash
# Unique users who sent tokens in the last week
cx metrics query 'count(count by (user_email) (increase(gemini_cli_token_usage_total[7d]) > 0))'
```

```promql
count(count by (user_email) (increase(gemini_cli_token_usage_total{<filters>}[<range>]) > 0))
```

### Token Usage

```bash
# Total tokens broken down by user (top 10)
cx metrics query 'topk(10, sum by (user_email) (increase(gemini_cli_token_usage_total[7d])))'
```

```promql
sum(increase(gemini_cli_token_usage_total{<filters>}[<range>]))
```

```promql
sum(increase(gemini_cli_token_usage_total{type="input",<filters>}[<range>]))
```

```promql
sum(increase(gemini_cli_token_usage_total{type="output",<filters>}[<range>]))
```

```promql
sum(increase(gemini_cli_token_usage_total{type="cache",<filters>}[<range>]))
```

```promql
topk(<N>, sum by (user_email) (increase(gemini_cli_token_usage_total{<filters>}[<range>])))
```

### Usage by Model

```promql
sum by (model) (increase(gemini_cli_api_request_count_total{<filters>}[<range>]))
```

```promql
sum by (model) (increase(gemini_cli_token_usage_total{<filters>}[<range>]))
```

### Top Programming Language

```promql
topk(1, sum by (programming_language) (increase(gemini_cli_file_operation_count_total{<filters>}[<range>])))
```

### Tool Calls

```bash
# Top tools by call count
cx metrics query 'topk(10, sum by (function_name) (increase(gemini_cli_tool_call_count_total[7d])))'
```

```promql
sum(increase(gemini_cli_tool_call_count_total{decision=~"accept|auto_accept",<filters>}[<range>]))
```

```promql
topk(<N>, sum by (function_name) (increase(gemini_cli_tool_call_count_total{<filters>}[<range>])))
```
