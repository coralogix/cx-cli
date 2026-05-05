# Span Querying Reference

Query and analyze distributed tracing data using the `cx spans` command with DataPrime syntax.

> **DataPrime syntax:** See `dataprime-reference.md` for the full query language reference.

## Understanding Spans in Coralogix

Spans are the fundamental unit of tracing data. **Traces are not stored as single entities** - they are logical groupings of spans that share the same `traceID`. To analyze a trace, you query its constituent spans.

This means:
- **Metadata (`$m.*`)** and **labels (`$l.*`)** are predictable - you can always filter on timestamp, duration, service name, and operation name without discovery.
- **User data (`$d.*`)** contains trace identifiers (`traceID`, `spanID`, `parentSpanID`) and application-specific tags/attributes that vary by service. Always verify custom `$d` fields before assuming they exist.

---

## CLI Command

```bash
cx spans '<dataprime_query>'
```

The `source spans` is automatically injected - do not include it in the query.

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--start` | `now-1h` | Start time (ISO 8601 or relative, e.g. `now-6h`) |
| `--end` | `now` | End time |
| `--limit` | `200` | Maximum number of results |
| `--tier` | `frequent` | Storage tier: `frequent` (hot/recent) or `archive` (cold/historical) |
| `-o, --output` | `text` | Output format: `text`, `json`, or `agents` |

---

## Span Data Model

### Standard Fields (Always Available)

| Field | Description |
|-------|-------------|
| `$m.timestamp` | Span start timestamp |
| `$m.duration` | Span duration in **microseconds** (see [Duration Units](#duration-units)) |
| `$l.applicationName` | Application name - highest-level label. Meaning varies by customer (environment, team, region) but it always exists. |
| `$l.subsystemName` | Subsystem name - second-level label. Typically maps to a component. |
| `$l.serviceName` | Service name - the logical service unit emitting the span. |
| `$l.operationName` | Operation name - the span title (e.g. "POST /checkout", "db.query"). |
| `$d.traceID` | Trace ID - groups spans into a single trace. |
| `$d.spanID` | Unique span identifier. |
| `$d.parentSpanID` | Parent span ID (empty string for root spans). |
| `$d.*` | Application-specific tags and attributes (see [Field Discovery](#field-discovery)). |

> **Note on label fields:** The meaning of `$l.applicationName` and `$l.subsystemName` varies by customer - they may represent environments, teams, regions, or something else entirely. Don't assume what they map to. Use `cx search-fields` or sample queries to verify actual values.

### Duration Units

`$m.duration` is in **microseconds**:
- 500ms = `500000`
- 1s = `1000000`
- 1min = `60000000`

When presenting duration values, always convert to human-readable units (milliseconds, seconds, or minutes) and include the unit. Never display raw microsecond values or the "µs" symbol.

```dataprime
# Computed field for milliseconds
create latency_ms from $m.duration / 1000
```

### Error Detection

Spans do not have a `$m.severity` field like logs. Errors are typically indicated by:
- `$d.tags.error == true` - the most common convention (OpenTelemetry/Jaeger)
- Status codes in custom fields (e.g. `$d.http.status_code`, `$d.grpc.status_code`)
- Other application-specific error tags

The exact field depends on the instrumentation library used. If `$d.tags.error` returns no results, inspect sample spans with `-o json` to discover how errors are tagged:

```bash
cx spans "filter \$l.serviceName == 'api'" --limit 5 -o json
```

---

## Essential Query Examples

```bash
# Get all spans for a trace
cx spans "filter \$d.traceID == '4f6a8f3c2e8a1b97'"

# Find spans for a service
cx spans "filter \$l.serviceName == 'checkout-service'"

# Find slow spans (> 1 second)
cx spans "filter \$m.duration > 1000000"

# Find error spans
cx spans "filter \$d.tags.error == true"

# Aggregate latency by operation
cx spans "groupby \$l.operationName aggregate avg(\$m.duration) as avg_latency | orderby avg_latency desc"

# Wider time range
cx spans "filter \$l.serviceName == 'api'" --start now-6h
```

### Wildfind Policy

**Avoid `wildfind` by default.** It scans all fields and is expensive.

The **one exception**: when the user provides a specific string and you don't know which field contains it:

```bash
cx spans "wildfind 'connection refused'"
```

> **Tip:** `wildfind` can also serve as a last-resort field discovery method - when `cx search-fields` doesn't find what you need, run `wildfind` with a known value, then inspect the matching spans to see which fields contain it.

---

## Field Discovery

**Skip discovery when:**
- The query only uses standard fields (`$m.duration`, `$l.serviceName`, `$l.operationName`, `$d.traceID`)
- The user explicitly names the fields they want
- The fields have already been discovered earlier in the conversation

### 1. Infer from Source Code (Preferred)

If you have access to the application's source code, examine OpenTelemetry instrumentation, span attribute definitions, and tracing middleware to identify field names directly.

### 2. Semantic Search

```bash
cx search-fields "customer identifier" --dataset spans
cx search-fields "order ID" --dataset spans
cx search-fields "http response code" --dataset spans
```

Note: `cx search-fields` only has access to the most common fields. If it doesn't find what you need, fall back to sample query inspection.

### 3. Sample Query Inspection

```bash
cx spans "filter \$l.serviceName == 'api'" --limit 5 -o json
```

Inspect the JSON output to see all available fields. Especially useful for discovering fields in unstructured or deeply nested data.

---

## Investigation Workflow

### 1. Understand the Request

Identify:
- Whether you have a trace ID, service name, or error description
- Time frame of interest
- Whether the question is about latency, errors, or request flow

### 2. Start with Known Information

**If you have a trace ID** - go straight to it:
```bash
cx spans "filter \$d.traceID == '<trace_id>'"
```

**If you have a service name** - query its spans:
```bash
cx spans "filter \$l.serviceName == '<service>'" --limit 50
```

**If you have neither** - start broad to find entry points:
```bash
# Find recent error spans
cx spans "filter \$d.tags.error == true" --limit 20

# Find the slowest spans in the last hour
cx spans "groupby \$l.serviceName, \$l.operationName aggregate avg(\$m.duration) as avg_latency | orderby avg_latency desc | limit 10"

# Then extract trace IDs from interesting spans
cx spans "filter \$l.serviceName == '<service>' && \$m.duration > 1000000 | distinct \$d.traceID"
```

### 3. Troubleshooting

If a query returns no results, change **one thing at a time**:

1. **Extend the time range**: `--start now-6h` or `--start now-24h`
2. **Relax filters**: remove the most restrictive condition
3. **Check field availability**: the field you're filtering by may only exist in a subset of spans
4. **Verify field names**: run a sample query with `-o json` to inspect the actual schema
5. **Check service names**: service names are case-sensitive
6. **Try archive tier**: `--tier archive --start now-30d` for older data

---

## Common Query Patterns

### Trace Reconstruction

```bash
# All spans for a trace
cx spans "filter \$d.traceID == '4f6a8f3c2e8a1b97'"

# Find root spans only (no parent)
cx spans "filter \$l.serviceName == 'api-gateway' | filter \$d.parentId == null"

# Find trace IDs for a service
cx spans "filter \$l.serviceName == 'payment-service' | distinct \$d.traceID"
```

### Latency Analysis

```bash
# Spans slower than 1 second
cx spans "filter \$m.duration > 1000000"

# Top 10 slowest operations by average duration
cx spans "groupby \$l.operationName aggregate avg(\$m.duration) as avg_latency | orderby avg_latency desc | limit 10"

# Average latency by service
cx spans "groupby \$l.serviceName aggregate avg(\$m.duration) as avg_latency"

# P95 latency by operation
cx spans "groupby \$l.operationName aggregate percentile(0.95, \$m.duration) as p95_latency"
```

### Latency Spike Detection

```bash
# Average latency per 15-minute window
cx spans "filter \$l.serviceName == 'api' | groupby roundTime(\$m.timestamp, 15m) as interval aggregate avg(\$m.duration) as avg_latency | orderby interval"

# Find the time windows with highest latency
cx spans "filter \$l.serviceName == 'api' | groupby roundTime(\$m.timestamp, 5m) as interval aggregate avg(\$m.duration) as avg_latency | orderby avg_latency desc | limit 10"
```

### Error Investigation

```bash
# All error spans
cx spans "filter \$d.tags.error == true"

# Error spans for a specific service
cx spans "filter \$l.serviceName == 'checkout' | filter \$d.tags.error == true"

# Error rate by service
cx spans "filter \$d.tags.error == true | groupby \$l.serviceName aggregate count() as errors | orderby errors desc"

# Error rate over time
cx spans "filter \$d.tags.error == true | groupby roundTime(\$m.timestamp, 15m) as interval aggregate count() as errors"
```

### Sampling Error Types

```bash
# Group errors by operation with a sample
cx spans "filter \$d.tags.error == true | groupby \$l.operationName aggregate any_value(\$d) as sample, count() as total | orderby total desc | limit 5"

# Group by service and operation to see where errors concentrate
cx spans "filter \$d.tags.error == true | groupby \$l.serviceName, \$l.operationName aggregate count() as errors | orderby errors desc | limit 10"
```

### Finding Unique Values

```bash
# List all services with spans
cx spans "distinct \$l.serviceName"

# List all operations for a service
cx spans "filter \$l.serviceName == 'api' | distinct \$l.operationName"

# Find unique trace IDs for error spans
cx spans "filter \$d.tags.error == true | distinct \$d.traceID"
```

### Correlating by Trace ID

```bash
# Find spans across services for the same trace
cx spans "filter \$d.traceID == 'abc123' | groupby \$l.serviceName aggregate count() as span_count, avg(\$m.duration) as avg_latency"
```

---

## Performance Tips

- Use `--limit` for exploratory queries
- Use `groupby` with aggregations instead of fetching raw spans when possible
- Filter by time first when dealing with large datasets
- Use specific filters (service name, operation) to reduce scan scope
- Don't rely solely on aggregations - retrieve sample spans to find information you didn't anticipate
- For large result sets, use `--output agents` which spills automatically:

```bash
cx spans "filter \$l.serviceName == 'api'" --start now-24h --limit 1000 -o agents
```
