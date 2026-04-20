---
name: query-spans
description: |
  Query and analyze distributed traces and spans using DataPrime syntax. Use this skill whenever the
  user wants to investigate request latency, find slow operations, debug service-to-service calls,
  look up a trace ID, analyze span durations, check error spans, examine distributed traces,
  investigate OpenTelemetry/Jaeger tracing data, or query Coralogix spans in any way — even if they
  don't explicitly mention "DataPrime" or "cx spans".
version: 0.1.0
---

# Spans Query Skill

Query and analyze distributed tracing data using the `cx spans` command with DataPrime syntax.

## Understanding Spans in Coralogix

Spans are the fundamental unit of tracing data. **Traces are not stored as single entities** — they are logical groupings of spans that share the same `traceID`. To analyze a trace, you query its constituent spans.

This means:
- **Metadata (`$m.*`)** and **labels (`$l.*`)** are predictable — you can always filter on timestamp, duration, service name, and operation name without discovery.
- **User data (`$d.*`)** contains trace identifiers (`traceID`, `spanID`, `parentSpanID`) and application-specific tags/attributes that vary by service. Always verify custom `$d` fields before assuming they exist.

---

## CLI Command

```bash
cx spans '<dataprime_query>'
```

The `source spans` is automatically injected — do not include it in the query.

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
| `$l.applicationName` | Application name — highest-level label. Meaning varies by customer (environment, team, region) but it always exists. |
| `$l.subsystemName` | Subsystem name — second-level label. Typically maps to a component. |
| `$l.serviceName` | Service name — the logical service unit emitting the span. |
| `$l.operationName` | Operation name — the span title (e.g. "POST /checkout", "db.query"). |
| `$d.traceID` | Trace ID — groups spans into a single trace. |
| `$d.spanID` | Unique span identifier. |
| `$d.parentSpanID` | Parent span ID (empty string for root spans). |
| `$d.*` | Application-specific tags and attributes (see [Field Discovery](#field-discovery)). |

> **Note on label fields:** The meaning of `$l.applicationName` and `$l.subsystemName` varies by customer — they may represent environments, teams, regions, or something else entirely. Don't assume what they map to. Use `cx search-fields` or sample queries to verify actual values.

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
- `$d.tags.error == true` — the most common convention (OpenTelemetry/Jaeger)
- Status codes in custom fields (e.g. `$d.http.status_code`, `$d.grpc.status_code`)
- Other application-specific error tags

The exact field depends on the instrumentation library used. If `$d.tags.error` returns no results, inspect sample spans with `-o json` to discover how errors are tagged:

```bash
cx spans 'filter $l.serviceName == "api"' --limit 5 -o json
```

---

## Querying Spans

### Essential Examples

```bash
# Get all spans for a trace
cx spans 'filter $d.traceID == "4f6a8f3c2e8a1b97"'

# Find spans for a service
cx spans 'filter $l.serviceName == "checkout-service"'

# Find slow spans (> 1 second)
cx spans 'filter $m.duration > 1000000'

# Find error spans
cx spans 'filter $d.tags.error == true'

# Aggregate latency by operation
cx spans 'groupby $l.operationName aggregate avg($m.duration) as avg_latency | orderby avg_latency desc'

# Wider time range
cx spans 'filter $l.serviceName == "api"' --start now-6h
```

### Wildfind Policy

**Avoid `wildfind` by default.** It scans all fields and is expensive.

The **one exception**: when the user provides a specific string and you don't know which field contains it:

```bash
cx spans 'wildfind "connection refused"'
```

In all other cases, use `filter` with known fields or discover field names first with `cx search-fields`.

> **Tip:** `wildfind` can also serve as a last-resort field discovery method — when `cx search-fields` doesn't find what you need, run `wildfind` with a known value, then inspect the matching spans to see which fields contain it.

---

## Field Discovery

**Skip discovery when:**
- The query only uses standard fields (`$m.duration`, `$l.serviceName`, `$l.operationName`, `$d.traceID`)
- The user explicitly names the fields they want
- The fields have already been discovered earlier in the conversation

For customer-specific `$d.*` fields that need discovery, use one of these approaches:

### 1. Infer from Source Code (Preferred)

If you have access to the application's source code, examine OpenTelemetry instrumentation, span attribute definitions, and tracing middleware to identify field names directly.

### 2. Semantic Search

```bash
cx search-fields "customer identifier" --dataset spans
cx search-fields "order ID" --dataset spans
cx search-fields "http response code" --dataset spans
```

Returns DataPrime paths with similarity scores. Note: `cx search-fields` only has access to the most common fields. If it doesn't find what you need, fall back to sample query inspection.

### 3. Sample Query Inspection

```bash
cx spans 'filter $l.serviceName == "api"' --limit 5 -o json
```

Inspect the JSON output to see all available fields in the actual data. This is especially useful for discovering fields in unstructured or deeply nested data that semantic search may not cover.

---

## Troubleshooting

If a query returns no results, change **one thing at a time**:

1. **Extend the time range**: `--start now-6h` or `--start now-24h`
2. **Relax filters**: remove the most restrictive condition
3. **Check field availability**: the field you're filtering by may only exist in a subset of spans — a span matching one filter may not have the field from another filter
4. **Verify field names**: run a sample query with `-o json` to inspect the actual schema
5. **Check service names**: service names are case-sensitive
6. **Try archive tier**: `--tier archive --start now-30d` for older data

---

## References

- **`dataprime` skill** — Full query language reference: commands, operators, aggregations, text extraction, type conversions
- **[Advanced Usage](references/advanced-usage.md)** — Investigation workflows, common query patterns, latency analysis, trace debugging

For inline DataPrime help:

```bash
cx dataprime list                  # List all commands and functions
cx dataprime show filter           # Detailed help for a specific command
```

---

## Related Skills

- **`metrics-query`** — Aggregated latency metrics, histograms, counters (PromQL)
- **`query-logs`** — Detailed log messages correlated with spans (DataPrime)
- **`telemetry-querying`** — Gateway skill for choosing the right data source
