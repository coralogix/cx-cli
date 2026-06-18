# Cursor Reference

> **DataPrime syntax:** See `dataprime-reference.md` for the query language reference.
> **Span querying:** See `spans-querying.md` for the span data model and filtering patterns.

Cursor uses **DataPrime spans** exclusively. Cursor does not emit provider token counts — `cursor.prompt_len` is prompt text length and must not be merged into cross-agent token totals.

---

## Span Data Model

Base source filter:

```text
source spans
| filter $l.serviceName == 'cursor-agent'
```

| Tag / Label | Description |
|---|---|
| `$l.serviceName` | `cursor-agent` |
| `$l.operationName` | Operation type (e.g. `cursor.beforeSubmitPrompt`, `cursor.afterFileEdit`) |
| `tags['cursor.conversation_id']` | Session / conversation identifier |
| `tags['cursor.user_email']` | User email |
| `tags['cursor.prompt_len']` | Prompt text length (not provider token count) |
| `tags['cursor.lines_added']` | Lines added in a file edit |
| `tags['cursor.lines_deleted']` | Lines deleted in a file edit |
| `tags['gen_ai.request.model']` | Model name |
| `tags['gen_ai.tool.name']` | Tool name |
| `$l.applicationName` / `$l.subsystemName` | App/subsystem labels (mixed case for spans) |

Key operations:
- `cursor.beforeSubmitPrompt` — carries `cursor.prompt_len`
- `cursor.afterFileEdit` — carries `cursor.lines_added` and `cursor.lines_deleted`

---

## Span Queries

### Total Unique Sessions

```bash
cx spans 'filter $l.serviceName == "cursor-agent"
| filter tags["cursor.conversation_id"] != null
| aggregate distinct_count(tags["cursor.conversation_id"]) as totalSessions
| choose totalSessions' --start now-7d -o agents
```

```text
source spans
| filter $l.serviceName == 'cursor-agent'
| filter tags['cursor.conversation_id'] != null
| aggregate distinct_count(tags['cursor.conversation_id']) as totalSessions
| choose totalSessions
```

### Unique Users

```text
source spans
| filter $l.serviceName == 'cursor-agent'
| filter tags['cursor.user_email'] != null
| aggregate distinct_count(tags['cursor.user_email']) as uniqueUsers
| choose uniqueUsers
```

### Total Runtime (ms)

```text
source spans
| filter $l.serviceName == 'cursor-agent'
| filter tags['cursor.conversation_id'] != null
| groupby tags['cursor.conversation_id'] as conversationId aggregate min($m.timestamp) as firstTs, max($m.timestamp) as lastTs
| create durationNs from (lastTs - firstTs):number
| create durationMs from durationNs / 1000000
| aggregate sum(durationMs) as totalRuntimeMs
| choose totalRuntimeMs
```

### Prompt Input Length Over Time

```bash
# Replace <interval>ms with e.g. 3600000 for 1-hour buckets
cx spans 'filter $l.serviceName == "cursor-agent"
| filter $l.operationName == "cursor.beforeSubmitPrompt"
| create promptLength from tags["cursor.prompt_len"]:number
| groupby roundTime($m.timestamp, <interval>ms) as Time aggregate sum(promptLength) as totalInput
| sort by Time asc
| choose Time, totalInput' --start now-7d -o agents
```

```text
source spans
| filter $l.serviceName == 'cursor-agent'
| filter $l.operationName == 'cursor.beforeSubmitPrompt'
| create promptLength from tags['cursor.prompt_len']:number
| groupby roundTime($m.timestamp, <interval>ms) as Time aggregate sum(promptLength) as totalInput
| sort by Time asc
| choose Time, totalInput
```

### Top Users by Lines Added

```text
source spans
| filter $l.serviceName == 'cursor-agent'
| filter $l.operationName == 'cursor.afterFileEdit'
| filter tags['cursor.user_email'] != null
| create user from tags['cursor.user_email']
| create linesAdded from tags['cursor.lines_added']:number
| groupby user aggregate sum(linesAdded) as totalLinesAdded
| sort by totalLinesAdded desc
| limit <N>
| choose user as userEmail, totalLinesAdded
```

### Code Impact (Lines Added / Removed)

```text
source spans
| filter $l.serviceName == 'cursor-agent'
| filter $l.operationName == 'cursor.afterFileEdit'
| create linesAdded from tags['cursor.lines_added']:number
| create linesDeleted from tags['cursor.lines_deleted']:number
| aggregate sum(linesAdded) as totalLinesAdded, sum(linesDeleted) as totalLinesRemoved
| choose totalLinesAdded, totalLinesRemoved
```

### Sessions by Model

```text
source spans
| filter $l.serviceName == 'cursor-agent'
| filter tags['gen_ai.request.model'] != null
| filter tags['gen_ai.request.model'] != 'unknown'
| create model from tags['gen_ai.request.model']
| groupby model aggregate count() as spanCount
| sort by spanCount desc
| limit <N>
| choose model, spanCount
```

### Top Tools by Call Count

```text
source spans
| filter $l.serviceName == 'cursor-agent'
| filter tags['gen_ai.tool.name'] != null
| create toolName from tags['gen_ai.tool.name']
| groupby toolName aggregate count() as callCount
| sort by callCount desc
| limit <N>
| choose toolName, callCount
```

### User Drilldown

Append to any query above to scope to a specific user:

```text
| filter tags['cursor.user_email'] == '<user_email>'
```
