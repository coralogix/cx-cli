---
name: logs-querying
description: Use when the user asks to "query logs", "find errors in logs", "search log messages", "investigate log data", "check application logs", "find stack traces", "debug using logs", "analyze log patterns", "find log entries", "check error logs", "search for exceptions", "look at logs", "filter logs by severity", "find warning messages", "check what happened in logs", or wants to query Coralogix logs using DataPrime syntax.
version: 0.1.0
---

# Logs Querying Skill

Use this skill to query and analyze Coralogix logs using DataPrime syntax. Logs are the primary source of detailed application output, error messages, stack traces, and event records.

## CLI Command

All log queries use the `cx logs` command with DataPrime syntax:

```bash
cx logs '<dataprime_query>'
```

The `source logs` prefix is automatically added if not present.

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--start` | `now-1h` | Start time (ISO 8601 or relative) |
| `--end` | `now` | End time (ISO 8601 or relative) |
| `--limit` | `100` | Maximum number of results |
| `--tier` | `frequent` | Storage tier: `frequent` or `archive` |
| `-o, --output` | `text` | Output format: `text`, `json`, or `agents` |

### Examples

```bash
# Find error logs
cx logs 'filter $m.severity == ERROR'

# Search for specific text in messages
cx logs 'filter $d.message ~ "timeout"'

# Filter by application and subsystem
cx logs 'filter $l.applicationname == "api" && $l.subsystemname == "auth"'

# Get logs from a wider time range
cx logs 'filter $m.severity == ERROR' --start now-6h --end now

# Query archive tier for historical data
cx logs 'filter $d.user_id == "12345"' --tier archive --start now-7d
```

---

## Log Data Model

### Standard Fields (Always Available)

These fields are always present — no discovery needed:

| Field | Description |
|-------|-------------|
| `$m.timestamp` | Log timestamp |
| `$m.severity` | Log severity: DEBUG, INFO, WARNING, ERROR, CRITICAL |
| `$m.templateid` | JSON schema identifier for the log (if applicable) |
| `$l.applicationname` | Application name (environment identifier) |
| `$l.subsystemname` | Subsystem name (component identifier) |
| `$d.*` | User data fields (customer-specific) |

### Severity Values

The `$m.severity` field uses special keywords **without quotes**:
- `DEBUG`
- `INFO`
- `WARNING`
- `ERROR`
- `CRITICAL`

Example:
```bash
# Correct - no quotes around severity value
cx logs 'filter $m.severity == ERROR'

# Also correct - filter multiple severities
cx logs 'filter [ERROR, CRITICAL].arrayContains($m.severity)'
```

### Customer-Specific Fields

Customer data varies. Use `cx search-fields` to discover field names:

```bash
cx search-fields "customer identifier" --dataset logs
cx search-fields "order ID" --dataset logs
cx search-fields "error message" --dataset logs --limit 10
```

---

## DataPrime Syntax Reference

### Query Structure

DataPrime queries are a sequence of commands separated by `|`. Each command's output feeds into the next:

```dataprime
source logs | filter $m.severity == ERROR | limit 10
```

### Data Prefixes

| Prefix | Description |
|--------|-------------|
| `$d` | User data (default, can be omitted) |
| `$l` | User labels |
| `$m` | Metadata |

### Basic Commands

| Command | Description | Example |
|---------|-------------|---------|
| `filter` | Filter rows by condition | `filter $m.severity == ERROR` |
| `groupby` | Group and aggregate | `groupby $l.subsystemname aggregate count()` |
| `orderby` | Sort results | `orderby $d.timestamp desc` |
| `limit` | Limit result count | `limit 10` |
| `distinct` | Unique values | `distinct $l.subsystemname` |
| `create` | Create computed field | `create error_rate from $d.errors / $d.total` |
| `choose` | Select specific fields | `choose $m.timestamp, $d.message` |

### Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `==` | Equals | `filter $m.severity == ERROR` |
| `!=` | Not equals | `filter $l.subsystemname != 'test'` |
| `>`, `<`, `>=`, `<=` | Comparison | `filter $d.response_time > 1000` |
| `~` | Contains (fuzzy text search) | `filter $d.message ~ 'timeout'` |
| `&&` | AND | `filter $m.severity == ERROR && $l.applicationname == 'api'` |
| `\|\|` | OR | `filter $m.severity == ERROR \|\| $m.severity == CRITICAL` |

### Type Conversions

Convert field types inline:
```dataprime
filter $d.http_error_code:number == 500
```

Supported types: `bool`, `number`, `string`, `timestamp`, `interval`, `array`, `object`

### Field Access

```dataprime
# Chained field names
filter $d.tags.user_context.email == 'test@example.com'

# Special characters - use brackets
filter $d.http['status/code'] == 500
```

### Aggregations

| Function | Description |
|----------|-------------|
| `count()` | Count rows |
| `sum($field)` | Sum values |
| `avg($field)` | Average |
| `min($field)` | Minimum |
| `max($field)` | Maximum |
| `percentile(0.95, $field)` | Percentile |
| `distinct_count($field)` | Count unique values |
| `any_value($field)` | Random sample value |

Example:
```bash
cx logs 'groupby $l.subsystemname aggregate count() as error_count, avg($d.response_time) as avg_response | orderby error_count desc'
```

### Time-Based Grouping

```bash
# Group by hour
cx logs 'groupby roundTime($m.timestamp, 1h) as hour aggregate count() as count'

# Error rate over 15-minute intervals
cx logs 'filter $m.severity == ERROR | groupby roundTime($m.timestamp, 15m) as interval aggregate count() as errors'
```

### Multi-Value Matching

```bash
# Match against multiple values
cx logs 'filter ["api", "web", "worker"].arrayContains($l.subsystemname)'

# Match multiple severity levels
cx logs 'filter [ERROR, CRITICAL].arrayContains($m.severity)'
```

### Text Extraction

#### Regex Extraction
```bash
# Extract email domain
cx logs 'extract $d.email into domain using regexp(e=/@(.*)/) | distinct $d.domain._0'

# Named capture groups
cx logs 'extract $d.email into extracted using regexp(e=/(?<username>[a-zA-Z0-9._%+-]+)@(?<domain>.*)/) | choose $d.extracted.username, $d.extracted.domain'
```

#### JSON String Parsing
```bash
# Parse JSON string field into object
cx logs 'extract $d.json_payload into parsed using jsonobject() | filter $d.parsed.status == "failed"'
```

### Deduplication

```bash
# Remove duplicates by template
cx logs 'dedupeby $m.templateid'

# Dedupe by custom field
cx logs 'dedupeby $d.request_id'
```

### Wildfind

Search across all fields for a specific string:

```bash
cx logs 'wildfind "connection refused"'
```

**When to use wildfind first:**
- When searching for a **specific log message** the user provided (e.g., "login successful", "payment processed", "connection refused")
- When you know the exact text but not which field contains it
- When the user quotes a specific error message or log output

```bash
# User asks: "Find logs with 'authentication failed'"
cx logs 'wildfind "authentication failed"'

# User asks: "Show me the 'order completed' logs"
cx logs 'wildfind "order completed"'
```

**When to use other methods first:**
- When exploring or aggregating logs (use `filter` with standard fields)
- When you need to filter by field type rather than specific text (use `cx search-fields` to discover fields)
- When building complex queries with multiple conditions

**Note:** Wildfind searches ALL fields, which can return noisy results for generic terms. For specific, quoted messages from the user, it's the fastest path to results.

### Fetching Sample Logs by Template

Find top error patterns with sample messages:
```bash
cx logs 'filter $m.severity == ERROR | groupby $m.templateid aggregate any_value($d) as sample, count() as total | orderby total desc | limit 5'
```

---

## Field Discovery

### When to Discover Fields

**Skip discovery for standard fields:**
- `$m.severity`, `$m.timestamp` — always available
- `$l.applicationname`, `$l.subsystemname` — always available

**Use discovery for customer-specific fields:**
- User identifiers, order IDs, transaction amounts
- Custom application fields
- Domain-specific data

### Discovery Methods

#### 1. Infer from Code (Preferred when available)

If you have access to the application's source code, you can infer log field names directly by examining:
- Logger calls (e.g., `logger.info("message", {"user_id": ...})`)
- Structured logging configurations
- Log formatting templates
- Field names in log statements

This is often faster and more accurate than semantic search, as it shows you exactly what fields the application emits.

#### 2. Using cx search-fields

When code is not available, use semantic search:

```bash
# Search by semantic description
cx search-fields "customer identifier" --dataset logs
cx search-fields "http response code" --dataset logs
cx search-fields "error message" --dataset logs --limit 10
```

The command returns DataPrime paths you can use directly in queries:

```
+------------------------+-----------------------------------+-----------+
| DataPrime path         | Description                       | Similarity|
+------------------------+-----------------------------------+-----------+
| $d.customer_id         | Unique customer identifier        | 0.89      |
| $d.user.account_id     | Customer account reference        | 0.85      |
+------------------------+-----------------------------------+-----------+
```

#### 3. Sample Query Inspection

Fetch sample logs and inspect the JSON structure:

```bash
cx logs 'filter $l.subsystemname == "api"' --limit 5 -o json
```

This reveals all available fields in the actual log data.

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
# Show recent errors - no discovery needed
cx logs 'filter $m.severity == ERROR | limit 20'

# Errors in a specific subsystem
cx logs 'filter $m.severity == ERROR && $l.subsystemname == "payment-service"'
```

### 3. Discover Fields When Needed

For customer-specific data:

```bash
# Find customer-related fields
cx search-fields "customer identifier" --dataset logs

# Find error message fields
cx search-fields "error message" --dataset logs
```

### 4. Build and Execute Query

Start simple, add complexity:

```bash
# Step 1: Check if data exists
cx logs 'filter $l.subsystemname == "checkout"' --limit 10

# Step 2: Add filters
cx logs 'filter $l.subsystemname == "checkout" && $m.severity == ERROR'

# Step 3: Add aggregation
cx logs 'filter $l.subsystemname == "checkout" && $m.severity == ERROR | groupby $d.error_type aggregate count() as occurrences'
```

### 5. Analyze and Iterate

If results are incomplete:
- Widen the time range
- Relax filters
- Try alternative field names
- Check for data in archive tier

### 6. Summarize Findings

After investigation, provide:
- Key insights from the logs
- Significant queries that led to findings
- Recommended next steps

---

## Troubleshooting

### Zero Results

If a query returns no results, troubleshoot **one change at a time**:

1. **Extend the time range** (unless user specified a specific time)
   ```bash
   # Try 6 hours instead of 1 hour
   cx logs 'filter $m.severity == ERROR' --start now-6h
   ```

2. **Relax the filters** - one at a time
   - Remove the most restrictive filter
   - Check if field names are correct
   - Verify values match the data format

3. **Check field availability**
   ```bash
   # Get sample logs to see available fields
   cx logs 'filter $l.subsystemname == "api"' --limit 5 -o json
   ```

4. **Try archive tier** for older data
   ```bash
   cx logs 'filter $d.order_id == "12345"' --tier archive --start now-30d
   ```

### Best Practices

- **Start simple**: Query first, discover later. For basic queries using standard fields, just run them.
- **Keep queries minimal**: Don't add `choose` or extra aggregations unless needed.
- **Verify assumptions**: Don't assume field names — verify with `cx search-fields` or sample queries.
- **One change at a time**: When debugging, modify one thing per query attempt.

---

## Common Query Patterns

### Error Investigation

```bash
# All errors in last hour
cx logs 'filter $m.severity == ERROR'

# Critical errors only
cx logs 'filter $m.severity == CRITICAL'

# Errors with text search
cx logs 'filter $m.severity == ERROR && $d.message ~ "database connection"'
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

### Correlating by ID

```bash
# Find all logs for a request ID
cx logs 'filter $d.request_id == "abc-123-def"'

# Find logs for a user
cx logs 'filter $d.user_id == "user_12345"' --start now-24h
```

---

## DataPrime Documentation

For complete DataPrime reference, use the built-in documentation:

```bash
# List all commands and functions
cx dataprime list

# Show help for a specific command
cx dataprime show filter
cx dataprime show groupby

# Search for functions by name
cx dataprime list --filter functions --name time
```

---

## Tips

### Performance

- Use `--limit` for exploratory queries
- Use `groupby` with aggregations instead of fetching all raw logs
- Filter by time first when dealing with large datasets
- Use specific filters (application, subsystem) to reduce scan scope

### Debugging a Production Issue

1. Start with recent errors: `cx logs 'filter $m.severity == ERROR' --limit 20`
2. Identify the affected service from log labels
3. Narrow to that service: `cx logs 'filter $l.subsystemname == "service-name" && $m.severity == ERROR'`
4. Search for patterns in error messages
5. Correlate with request IDs or user IDs
6. Check time patterns for spikes

### Working with Large Result Sets

For large queries, use `--output agents` which automatically spills to a temp file when results exceed the configured threshold:

```bash
cx logs 'filter $m.severity == ERROR' --start now-24h --limit 1000 -o agents
```

---

## Related Skills

- **`metrics-query`** — For aggregated counters, gauges, and histograms
- **`spans-query`** — For distributed traces and service latency
- **`telemetry-querying`** — Gateway skill for choosing the right data source
- **`cx-alerts`** — Creating alerts based on log patterns
