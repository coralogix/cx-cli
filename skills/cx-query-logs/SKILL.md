---
name: cx-query-logs
description: |
  Query and analyze Coralogix logs using DataPrime syntax. Use this skill whenever the user wants to
  search logs, find errors, investigate log data, debug application issues, check error logs, find
  stack traces, analyze log patterns, filter by severity, look up specific log entries, or query
  Coralogix logs in any way - even if they don't explicitly mention "DataPrime" or "cx logs".
version: 0.1.0
---

# Logs Querying Skill

Query and analyze Coralogix logs using the `cx logs` command with DataPrime syntax.

## Understanding Logs in Coralogix

Logs in Coralogix are **largely unstructured**. Every log entry has a small structured envelope - metadata and labels - but the actual application payload (`userData`) is free-form and varies entirely by application. There is no universal schema for `$d.*` fields.

This means:
- **Metadata (`$m.*`)** and **labels (`$l.*`)** are predictable - you can always filter on severity, timestamp, application name, and subsystem name without discovery.
- **User data (`$d.*`)** is not predictable - field names, nesting, and types depend on whatever the application chose to log. Always verify `$d` fields before assuming they exist.

---

## CLI Command

```bash
cx logs '<dataprime_query>'
```

The `source logs` prefix is automatically injected if the query doesn't already include a `source` command.

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--start` | `now-1h` | Start time (ISO 8601 or relative, e.g. `now-6h`) |
| `--end` | `now` | End time |
| `--limit` | `100` | Maximum number of results |
| `--tier` | `frequent` | Storage tier: `frequent` (hot/recent) or `archive` (cold/historical) |
| `-o, --output` | `text` | Output format: `text`, `json`, or `agents` |

---

## Log Data Model

### Standard Fields (Always Available)

| Field | Description |
|-------|-------------|
| `$m.timestamp` | Log timestamp |
| `$m.severity` | Severity level (see below) |
| `$m.templateid` | Log template identifier (groups structurally similar logs) |
| `$l.applicationname` | Application name - the highest-level label. All data in Coralogix is tagged with it. Meaning varies by customer (environment, team, region) but it always exists. |
| `$l.subsystemname` | Subsystem name - second highest-level label. All data is tagged with it. Typically maps to a service or component. |
| `$d.*` | User data - free-form, application-specific (see [Field Discovery](#field-discovery)) |

### Severity Values

Severity keywords are used **without quotes** in DataPrime:

`DEBUG` | `INFO` | `WARNING` | `ERROR` | `CRITICAL`

```bash
cx logs 'filter $m.severity == ERROR'
cx logs 'filter [ERROR, CRITICAL].arrayContains($m.severity)'
```

---

## Querying Logs

### Essential Examples

```bash
# Filter by severity
cx logs 'filter $m.severity == ERROR'

# Text search in a known field
cx logs "filter \$d.message ~ 'timeout'"

# Filter by application and subsystem
cx logs "filter \$l.applicationname == 'api' && \$l.subsystemname == 'auth'"

# Aggregate errors by subsystem
cx logs 'filter $m.severity == ERROR | groupby $l.subsystemname aggregate count() as errors | orderby errors desc'

# Wider time range and archive tier
cx logs "filter \$d.user_id == '12345'" --tier archive --start now-7d
```

### Wildfind Policy

**Avoid `wildfind` by default.** It scans all fields and returns noisy results, especially for generic terms.

The **one exception**: when the user provides a specific, quoted error message or log string and you don't know which field contains it:

```bash
# User says: "Find logs with 'connection refused'"
cx logs "wildfind 'connection refused'"
```

In all other cases, use `filter` with known fields (`$m.severity`, `$l.subsystemname`, `$d.<field>`) or discover field names first with `cx search-fields`.

---

## Field Discovery

**Skip discovery when:**
- The query only uses standard fields (`$m.severity`, `$m.timestamp`, `$l.applicationname`, `$l.subsystemname`)
- The user explicitly names the fields they want (e.g., "filter by `$d.customer_id`")
- You're searching for a specific error message - use `wildfind` directly
- The fields have already been discovered earlier in the conversation

For customer-specific `$d.*` fields that need discovery, use one of these approaches:

### 1. Infer from Source Code (Preferred)

If you have access to the application's source code, examine logger calls, structured logging configs, and log format templates to identify field names directly.

### 2. Semantic Search

```bash
cx search-fields "customer identifier" --dataset logs
cx search-fields "http response code" --dataset logs
```

Returns DataPrime paths with similarity scores:

```
+------------------------+-----------------------------------+-----------+
| DataPrime path         | Description                       | Similarity|
+------------------------+-----------------------------------+-----------+
| $d.customer_id         | Unique customer identifier        | 0.89      |
| $d.user.account_id     | Customer account reference        | 0.85      |
+------------------------+-----------------------------------+-----------+
```

### 3. Sample Query Inspection

```bash
cx logs "filter \$l.subsystemname == 'api'" --limit 5 -o json
```

Inspect the JSON output to see all available fields in the actual data.

---

## Troubleshooting

If a query returns no results, change **one thing at a time**:

1. **Extend the time range**: `--start now-6h` or `--start now-24h`
2. **Relax filters**: remove the most restrictive condition
3. **Verify field names**: run a sample query with `-o json` to inspect the actual schema
4. **Try archive tier**: `--tier archive --start now-30d` for older data

---

## References

- **`cx-dataprime` skill** - Full query language reference: commands, operators, aggregations, text extraction, type conversions
- **[Advanced Usage](references/advanced-usage.md)** - Investigation workflows, common query patterns, performance tips, production debugging

For inline DataPrime help:

```bash
cx dataprime list                  # List all commands and functions
cx dataprime show filter           # Detailed help for a specific command
```

---

## Related Skills

- **`cx-metrics-query`** - Aggregated counters, gauges, and histograms (PromQL)
- **`cx-query-spans`** - Distributed traces and service latency (DataPrime)
- **`cx-telemetry-querying`** - Gateway skill for choosing the right data source
- **`cx-alerts`** - Create and manage alerts based on log patterns
