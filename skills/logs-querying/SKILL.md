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

## DataPrime Reference

For the full DataPrime query language reference (commands, operators, aggregations, text extraction, etc.), see:

**[DataPrime Reference](../shared/references/dataprime-reference.md)**

For the built-in CLI documentation:

```bash
cx dataprime list                  # List all commands and functions
cx dataprime show filter           # Detailed help for a specific command
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

## Troubleshooting

### Zero Results

If a query returns no results, troubleshoot **one change at a time**:

1. **Extend the time range** (unless user specified a specific time)
   ```bash
   cx logs 'filter $m.severity == ERROR' --start now-6h
   ```

2. **Relax the filters** - one at a time
   - Remove the most restrictive filter
   - Check if field names are correct
   - Verify values match the data format

3. **Check field availability**
   ```bash
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

## Advanced Usage

For investigation workflows, common query patterns, performance tips, and production debugging strategies, see:

**[Advanced Usage](references/advanced-usage.md)**

---

## Related Skills

- **`metrics-query`** — For aggregated counters, gauges, and histograms
- **`spans-query`** — For distributed traces and service latency
- **`telemetry-querying`** — Gateway skill for choosing the right data source
- **`cx-alerts`** — Creating alerts based on log patterns
