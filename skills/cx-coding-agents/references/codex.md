# Codex Reference

> **DataPrime syntax:** See `dataprime-reference.md` for the query language reference.
> **Log querying:** See `logs-querying.md` for log data model and filtering patterns.
> **Span querying:** See `spans-querying.md` for span data model (latency queries only).

Codex uses **DataPrime logs** for most dashboard metrics (tokens, sessions, models, users, tools) and **DataPrime spans** only for `run_turn` latency.

---

## Log Data Model

Base source filter (covers both CLI and desktop services):

```text
source logs
| filter $d.resource.attributes['service.name']:string == 'codex_cli_rs' || $d.resource.attributes['service.name']:string == 'codex-app-server'
```

Codex supports two log shapes. Use `firstNonNull($d.logRecord.attributes['<name>'], $d.attributes['<name>'])` to normalize across both.

| Attribute | Description |
|---|---|
| `event.name` | Event type (e.g. `codex.sse_event`, `codex.tool_result`) |
| `event.kind` | Event kind (e.g. `response.completed`) |
| `conversation.id` | Conversation (session) identifier |
| `user.email` | User email |
| `model` | Model name |
| `input_token_count` | Input tokens for a completed response |
| `output_token_count` | Output tokens for a completed response |
| `cached_token_count` | Cached tokens for a completed response |
| `tool_name` | Tool name (on `codex.tool_result` events) |
| `$l.applicationname` / `$l.subsystemname` | App/subsystem labels (lowercase for logs) |

Completed response filter:

```text
| filter firstNonNull($d.logRecord.attributes['event.name'], $d.attributes['event.name']) == 'codex.sse_event'
| filter firstNonNull($d.logRecord.attributes['event.kind'], $d.attributes['event.kind']) == 'response.completed'
```

---

## Log Queries

### Token Totals

```bash
cx logs 'filter $d.resource.attributes["service.name"]:string == "codex_cli_rs" || $d.resource.attributes["service.name"]:string == "codex-app-server"
| filter firstNonNull($d.logRecord.attributes["event.name"], $d.attributes["event.name"]) == "codex.sse_event"
| filter firstNonNull($d.logRecord.attributes["event.kind"], $d.attributes["event.kind"]) == "response.completed"
| create input from firstNonNull($d.logRecord.attributes["input_token_count"], $d.attributes["input_token_count"])
| create output from firstNonNull($d.logRecord.attributes["output_token_count"], $d.attributes["output_token_count"])
| create cached from firstNonNull($d.logRecord.attributes["cached_token_count"], $d.attributes["cached_token_count"])
| aggregate sum(input:number) as totalInput, sum(output:number) as totalOutput, sum(cached:number) as totalCached
| create totalTokens from totalInput + totalOutput
| choose totalTokens, totalInput, totalOutput, totalCached' \
  --start now-7d -o toon
```

```text
source logs
| filter $d.resource.attributes['service.name']:string == 'codex_cli_rs' || $d.resource.attributes['service.name']:string == 'codex-app-server'
| filter firstNonNull($d.logRecord.attributes['event.name'], $d.attributes['event.name']) == 'codex.sse_event'
| filter firstNonNull($d.logRecord.attributes['event.kind'], $d.attributes['event.kind']) == 'response.completed'
| create input from firstNonNull($d.logRecord.attributes['input_token_count'], $d.attributes['input_token_count'])
| create output from firstNonNull($d.logRecord.attributes['output_token_count'], $d.attributes['output_token_count'])
| create cached from firstNonNull($d.logRecord.attributes['cached_token_count'], $d.attributes['cached_token_count'])
| aggregate sum(input:number) as totalInput, sum(output:number) as totalOutput, sum(cached:number) as totalCached
| create totalTokens from totalInput + totalOutput
| choose totalTokens, totalInput, totalOutput, totalCached
```

### Unique Sessions

```bash
cx logs 'filter $d.resource.attributes["service.name"]:string == "codex_cli_rs" || $d.resource.attributes["service.name"]:string == "codex-app-server"
| filter firstNonNull($d.logRecord.attributes["event.name"], $d.attributes["event.name"]) == "codex.sse_event"
| filter firstNonNull($d.logRecord.attributes["event.kind"], $d.attributes["event.kind"]) == "response.completed"
| create conversation_id from firstNonNull($d.logRecord.attributes["conversation.id"], $d.attributes["conversation.id"])
| filter conversation_id != null
| aggregate distinct_count(conversation_id) as uniqueSessions
| choose uniqueSessions' --start now-7d -o toon
```

```text
source logs
| filter $d.resource.attributes['service.name']:string == 'codex_cli_rs' || $d.resource.attributes['service.name']:string == 'codex-app-server'
| filter firstNonNull($d.logRecord.attributes['event.name'], $d.attributes['event.name']) == 'codex.sse_event'
| filter firstNonNull($d.logRecord.attributes['event.kind'], $d.attributes['event.kind']) == 'response.completed'
| create conversation_id from firstNonNull($d.logRecord.attributes['conversation.id'], $d.attributes['conversation.id'])
| filter conversation_id != null
| aggregate distinct_count(conversation_id) as uniqueSessions
| choose uniqueSessions
```

### Top Users by Token Usage

```text
source logs
| filter $d.resource.attributes['service.name']:string == 'codex_cli_rs' || $d.resource.attributes['service.name']:string == 'codex-app-server'
| filter firstNonNull($d.logRecord.attributes['event.name'], $d.attributes['event.name']) == 'codex.sse_event'
| filter firstNonNull($d.logRecord.attributes['event.kind'], $d.attributes['event.kind']) == 'response.completed'
| create user from firstNonNull($d.logRecord.attributes['user.email'], $d.attributes['user.email'])
| create input from firstNonNull($d.logRecord.attributes['input_token_count'], $d.attributes['input_token_count'])
| create output from firstNonNull($d.logRecord.attributes['output_token_count'], $d.attributes['output_token_count'])
| filter user != null
| groupby user aggregate sum(input:number) as totalInput, sum(output:number) as totalOutput
| create totalTokens from totalInput + totalOutput
| sort by totalTokens desc
| limit <N>
| choose user as userEmail, totalTokens
```

### Sessions by Model

```text
source logs
| filter $d.resource.attributes['service.name']:string == 'codex_cli_rs' || $d.resource.attributes['service.name']:string == 'codex-app-server'
| filter firstNonNull($d.logRecord.attributes['event.name'], $d.attributes['event.name']) == 'codex.sse_event'
| filter firstNonNull($d.logRecord.attributes['event.kind'], $d.attributes['event.kind']) == 'response.completed'
| create model from firstNonNull($d.logRecord.attributes['model'], $d.attributes['model'])
| create conversation_id from firstNonNull($d.logRecord.attributes['conversation.id'], $d.attributes['conversation.id'])
| filter model != null && conversation_id != null
| groupby model aggregate distinct_count(conversation_id) as sessions
| sort by sessions desc
| choose model, sessions
```

### Top Tools by Usage

```text
source logs
| filter $d.resource.attributes['service.name']:string == 'codex_cli_rs' || $d.resource.attributes['service.name']:string == 'codex-app-server'
| filter firstNonNull($d.logRecord.attributes['event.name'], $d.attributes['event.name']) == 'codex.tool_result'
| create tool_name from firstNonNull($d.logRecord.attributes['tool_name'], $d.attributes['tool_name'])
| countby tool_name into tool_count desc
| limit <N>
```

### Total Session Runtime

```text
source logs
| filter $d.resource.attributes['service.name']:string == 'codex_cli_rs' || $d.resource.attributes['service.name']:string == 'codex-app-server'
| create conversation_id from firstNonNull($d.logRecord.attributes['conversation.id'], $d.attributes['conversation.id'])
| filter conversation_id != null
| groupby conversation_id aggregate max($m.timestamp) as session_end, min($m.timestamp) as session_start
| create session_duration from (session_end - session_start) / 1ms
| aggregate sum(session_duration) as total_ms
| choose total_ms
```

---

## Span Queries (Latency Only)

Use spans only for `run_turn` latency. Other metrics come from logs.

```bash
# Average run_turn latency over the last day
cx spans 'filter $l.serviceName == "codex_cli_rs" || $l.serviceName == "codex-app-server"
| filter $l.operationName == "run_turn"
| create dur_ms from $m.duration:number / 1000
| aggregate avg(dur_ms) as avg_ms' --start now-1d -o toon
```

```text
source spans
| filter $l.serviceName == 'codex_cli_rs' || $l.serviceName == 'codex-app-server'
| filter $l.operationName == 'run_turn'
| create dur_ms from $m.duration:number / 1000
| aggregate avg(dur_ms) as avg_ms
```
