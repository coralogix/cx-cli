# Log Querying Reference

Query and analyze Coralogix logs using the `cx logs` command with DataPrime syntax.

> **DataPrime syntax:** See `dataprime-reference.md` for the full query language reference.

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

## Essential Query Examples

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
cx logs "filter \$l.subsystemname == 'payments'" --tier archive --start now-7d
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

## Investigation Workflow

### 1. Understand the Request

Identify:
- What type of logs are needed (errors, info, specific events)
- Time frame of interest
- Key entities (services, users, transactions)

### 2. Start with Standard Fields

For basic queries, use standard fields directly:

```bash
# Recent errors - no discovery needed
cx logs 'filter $m.severity == ERROR | limit 20'

# Errors in a specific subsystem
cx logs "filter \$m.severity == ERROR && \$l.subsystemname == 'payment-service'"
```

### 3. Build and Execute Query

Start simple, add complexity:

```bash
# Step 1: Check if data exists
cx logs "filter \$l.subsystemname == 'checkout'" --limit 10

# Step 2: Add filters
cx logs "filter \$l.subsystemname == 'checkout' && \$m.severity == ERROR"

# Step 3: Add aggregation
cx logs "filter \$l.subsystemname == 'checkout' && \$m.severity == ERROR | groupby \$d.error_type aggregate count() as occurrences"
```

### 4. Troubleshooting

If a query returns no results, change **one thing at a time**:

1. **Extend the time range**: `--start now-6h` or `--start now-24h`
2. **Relax filters**: remove the most restrictive condition
3. **Verify field names**: run a sample query with `-o json` to inspect the actual schema
4. **Try archive tier**: `--tier archive --start now-30d` for older data

---

## Common Query Patterns

### Error Investigation

```bash
# All errors in last hour
cx logs 'filter $m.severity == ERROR'

# Critical errors only
cx logs 'filter $m.severity == CRITICAL'

# Errors with text search
cx logs "filter \$m.severity == ERROR && \$d.message ~ 'database connection'"
```

### Aggregation by Service

```bash
# Error count by subsystem
cx logs 'filter $m.severity == ERROR | groupby $l.subsystemname aggregate count() as errors | orderby errors desc'

# Error count by application and subsystem
cx logs 'filter $m.severity == ERROR | groupby $l.applicationname, $l.subsystemname aggregate count() as errors'
```

### Time-Based Analysis

```bash
# Errors per hour
cx logs 'filter $m.severity == ERROR | groupby roundTime($m.timestamp, 1h) as hour aggregate count() as count'

# Find error spikes in 5-minute windows
cx logs 'filter $m.severity == ERROR | groupby roundTime($m.timestamp, 5m) as interval aggregate count() as count | orderby count desc | limit 10'
```

### Finding Unique Values

```bash
# List all subsystems with errors
cx logs 'filter $m.severity == ERROR | distinct $l.subsystemname'

# List unique error types
cx logs 'filter $m.severity == ERROR | distinct $d.error_type'
```

### Fetching Sample Logs by Template

Find top error patterns with sample messages:

```bash
cx logs 'filter $m.severity == ERROR | groupby $m.templateid aggregate any_value($d) as sample, count() as total | orderby total desc | limit 5'
```

---

## Performance Tips

- Use `--limit` for exploratory queries
- Use `groupby` with aggregations instead of fetching all raw logs
- Filter by time first when dealing with large datasets
- Use specific filters (application, subsystem) to reduce scan scope
- For large result sets, use `--output agents` which spills to a temp file automatically:

```bash
cx logs 'filter $m.severity == ERROR' --start now-24h --limit 1000 -o agents
```
