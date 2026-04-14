---
name: spans-query
description: Use when investigating distributed traces, analyzing span latency, debugging service-to-service calls, querying tracing data, finding slow operations, investigating request flows, checking trace IDs, analyzing OpenTelemetry/Jaeger-style span data, or answering questions about service dependencies and request paths in Coralogix.
version: 0.1.0
---

# Spans Query Skill

Use this skill to query and analyze distributed tracing data in Coralogix. Spans are the fundamental unit of tracing data — a trace is simply a collection of spans that share the same `traceID`.

## Core Concept

**Traces are not stored as single entities.** In Coralogix, tracing data is stored as individual spans. A trace is a logical grouping of spans that share the same `traceID`. To analyze a trace, you query its constituent spans.

This means:
- To get a full trace: query spans filtered by `traceID`
- To find traces for a service: query spans filtered by `serviceName`
- To analyze latency: aggregate span durations

## CLI Command

All span queries use the `cx spans query` command with DataPrime syntax:

```bash
cx spans query '<dataprime_query>'
```

The `source spans` prefix is automatically added if not present.

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--start` | `now-1h` | Start time (ISO 8601 or relative) |
| `--end` | `now` | End time (ISO 8601 or relative) |
| `--limit` | `200` | Maximum number of results |
| `--tier` | `frequent` | Storage tier: `frequent` or `archive` |
| `-o, --output` | `text` | Output format: `text`, `json`, or `agents` |

### Examples

```bash
# Get all spans for a specific trace
cx spans query 'filter $d.traceID == "abc123def456"'

# Find spans for a service
cx spans query 'filter $l.serviceName == "checkout-service"'

# Find slow spans (duration > 1 second)
cx spans query 'filter $m.duration > 1000000'

# Aggregate latency by operation
cx spans query 'groupby $l.operationName aggregate avg($m.duration) as avg_latency'

# Find error spans
cx spans query 'filter $d.tags.error == true'

# Custom time range
cx spans query 'filter $l.serviceName == "api"' --start now-6h --end now
```

---

## Span Data Model

### Standard Fields (Always Available)

These fields are always present — no discovery needed:

| Field | Description |
|-------|-------------|
| `$m.timestamp` | Span start timestamp |
| `$m.duration` | Span duration in **microseconds** |
| `$l.applicationName` | Application name (environment identifier) |
| `$l.subsystemName` | Subsystem name (component identifier) |
| `$l.serviceName` | Service name (logical service unit) |
| `$l.operationName` | Operation name (span title, e.g., "POST /checkout") |
| `$d.traceID` | Trace ID — groups spans into a trace |
| `$d.spanID` | Unique span identifier |
| `$d.parentSpanID` | Parent span ID (empty for root span) |

### Duration Units

The `$m.duration` field is in **microseconds**. Convert for display:
- Milliseconds: `$m.duration / 1000`
- Seconds: `$m.duration / 1000000`
- Minutes: `$m.duration / 60000000`

Example: Filter spans slower than 500ms:
```bash
cx spans query 'filter $m.duration > 500000'
```

### Customer-Specific Fields

Customer data varies. Use `cx search-fields` to discover field names:

```bash
cx search-fields "customer identifier" --dataset spans
cx search-fields "order ID" --dataset spans
cx search-fields "transaction amount" --dataset spans
```

---

## Common Query Patterns

### Get All Spans for a Trace

```bash
cx spans query 'filter $d.traceID == "4f6a8f3c2e8a1b97"'
```

### Find Traces by Service

Query spans for a service, then identify unique trace IDs:

```bash
cx spans query 'filter $l.serviceName == "payment-service" | distinct $d.traceID'
```

### Find Root Spans Only

Root spans have no parent:

```bash
cx spans query 'filter $l.serviceName == "api-gateway" | filter $d.parentSpanID == ""'
```

### Find Slow Operations

```bash
# Spans slower than 1 second
cx spans query 'filter $m.duration > 1000000'

# Top 10 slowest operations by average duration
cx spans query 'groupby $l.operationName aggregate avg($m.duration) as avg_latency | orderby avg_latency desc | limit 10'
```

### Find Error Spans

```bash
# Spans with error tag
cx spans query 'filter $d.tags.error == true'

# Error spans for a specific service
cx spans query 'filter $l.serviceName == "checkout" | filter $d.tags.error == true'
```

### Service Latency Analysis

```bash
# Average latency by service
cx spans query 'groupby $l.serviceName aggregate avg($m.duration) as avg_latency'

# P95 latency by operation (if percentile is available)
cx spans query 'groupby $l.operationName aggregate percentile(0.95, $m.duration) as p95_latency'
```

### Time-Based Grouping

```bash
# Span count per hour
cx spans query 'groupby roundTime($m.timestamp, 1h) as hour aggregate count() as span_count'

# Error rate over time
cx spans query 'filter $d.tags.error == true | groupby roundTime($m.timestamp, 15m) as interval aggregate count() as errors'
```

### Multi-Service Trace Analysis

```bash
# Find spans across multiple services in a trace
cx spans query 'filter $d.traceID == "abc123" | groupby $l.serviceName aggregate count() as span_count, avg($m.duration) as avg_latency'
```

---

## Investigation Workflow

### 1. Start with Known Information

If you have a trace ID:
```bash
cx spans query 'filter $d.traceID == "<trace_id>"'
```

If you have a service name:
```bash
cx spans query 'filter $l.serviceName == "<service>"' --limit 50
```

### 2. Discover Field Names When Needed

For customer-specific fields:
```bash
cx search-fields "user identifier" --dataset spans
cx search-fields "request ID" --dataset spans
```

### 3. Narrow Down with Filters

Add filters incrementally:
```bash
# Start broad
cx spans query 'filter $l.serviceName == "api"'

# Add time filter
cx spans query 'filter $l.serviceName == "api" | filter $m.duration > 500000'

# Add operation filter
cx spans query 'filter $l.serviceName == "api" | filter $l.operationName ~ "checkout"'
```

### 4. Aggregate for Insights

```bash
# Count by operation
cx spans query 'filter $l.serviceName == "api" | groupby $l.operationName aggregate count() as span_count'

# Latency distribution
cx spans query 'filter $l.serviceName == "api" | groupby $l.operationName aggregate avg($m.duration) as avg, max($m.duration) as max'
```

---

## DataPrime Quick Reference

### Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `==` | Equals | `filter $l.serviceName == "api"` |
| `!=` | Not equals | `filter $d.tags.error != true` |
| `>`, `<`, `>=`, `<=` | Comparison | `filter $m.duration > 1000000` |
| `~` | Contains (substring) | `filter $l.operationName ~ "checkout"` |
| `&&` | AND | `filter $l.serviceName == "api" && $m.duration > 1000000` |
| `\|\|` | OR | `filter $d.tags.error == true \|\| $m.duration > 5000000` |

### Commands

| Command | Description | Example |
|---------|-------------|---------|
| `filter` | Filter rows | `filter $m.duration > 1000` |
| `groupby` | Group and aggregate | `groupby $l.serviceName aggregate count()` |
| `orderby` | Sort results | `orderby $m.duration desc` |
| `limit` | Limit results | `limit 10` |
| `distinct` | Unique values | `distinct $d.traceID` |
| `wildfind` | Search all fields | `wildfind 'error'` |
| `create` | Create computed field | `create latency_ms from $m.duration / 1000` |

### Aggregations

| Function | Description |
|----------|-------------|
| `count()` | Count rows |
| `sum($field)` | Sum values |
| `avg($field)` | Average |
| `min($field)` | Minimum |
| `max($field)` | Maximum |
| `percentile(0.95, $field)` | Percentile |

---

## Migration from Old Commands

If you're migrating from the old `cx traces` commands:

| Old Command | New Command |
|-------------|-------------|
| `cx traces get <trace_id>` | `cx spans query 'filter $d.traceID == "<trace_id>"'` |
| `cx traces search <service>` | `cx spans query 'filter $l.serviceName == "<service>"'` |

---

## Tips

### Zero Results?

- **Check field names**: Use `cx search-fields` to verify field paths
- **Widen time range**: Try `--start now-6h` or `--start now-24h`
- **Check service names**: Service names are case-sensitive
- **Verify trace ID format**: Ensure the trace ID is correct

### Performance

- Use `limit` to cap results for exploratory queries
- Use `groupby` with aggregations instead of fetching raw spans when possible
- Filter by time first when dealing with large datasets

### Debugging a Request

1. Get the trace ID from logs or headers
2. Fetch all spans: `cx spans query 'filter $d.traceID == "<id>"'`
3. Identify slow spans: look at duration
4. Check for errors: look for `$d.tags.error == true`
5. Follow the call chain using `parentSpanID`

---

## Related Skills

- **`metrics-query`** — For aggregated latency metrics (histograms, counters)
- **`query-logs`** — For detailed log messages correlated with spans
- **`telemetry-querying`** — Gateway skill for choosing the right data source
