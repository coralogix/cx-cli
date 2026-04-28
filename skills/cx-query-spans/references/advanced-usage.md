# Spans — Advanced Usage

## Investigation Workflow

### 1. Understand the Request

Identify:
- Whether you have a trace ID, service name, or error description
- Time frame of interest
- Whether the question is about latency, errors, or request flow

### 2. Start with Known Information

**If you have a trace ID** — go straight to it:
```bash
cx spans "filter \$d.traceID == '<trace_id>'"
```

**If you have a service name** — query its spans:
```bash
cx spans "filter \$l.serviceName == '<service>'" --limit 50
```

**If you have neither** — start broad to find entry points:
```bash
# Find recent error spans
cx spans 'filter $d.tags.error == true' --limit 20

# Find the slowest spans in the last hour
cx spans 'groupby $l.serviceName, $l.operationName aggregate avg($m.duration) as avg_latency | orderby avg_latency desc | limit 10'

# Then extract trace IDs from interesting spans
cx spans "filter \$l.serviceName == '<service>' && \$m.duration > 1000000 | distinct \$d.traceID"
```

### 3. Discover Fields When Needed

For customer-specific data:

```bash
cx search-fields "user identifier" --dataset spans
cx search-fields "request ID" --dataset spans
```

### 4. Narrow Down with Filters

Add filters incrementally:

```bash
# Step 1: Start broad
cx spans "filter \$l.serviceName == 'api'"

# Step 2: Add duration filter
cx spans "filter \$l.serviceName == 'api' | filter \$m.duration > 500000"

# Step 3: Add operation filter
cx spans "filter \$l.serviceName == 'api' | filter \$l.operationName ~ 'checkout'"
```

### 5. Aggregate for Insights

```bash
# Count by operation
cx spans "filter \$l.serviceName == 'api' | groupby \$l.operationName aggregate count() as span_count"

# Latency distribution
cx spans "filter \$l.serviceName == 'api' | groupby \$l.operationName aggregate avg(\$m.duration) as avg, max(\$m.duration) as max"
```

### 6. Summarize Findings

After investigation, provide:
- Root cause or narrowed-down suspects
- Key queries that led to findings
- Recommended next steps (e.g. check related logs, examine a specific service)
- Always include relevant application/subsystem/service names — this helps correlate with source code

---

## Common Query Patterns

### Trace Reconstruction

```bash
# All spans for a trace
cx spans "filter \$d.traceID == '4f6a8f3c2e8a1b97'"

# Find root spans only (no parent)
cx spans "filter \$l.serviceName == 'api-gateway' | filter \$d.parentSpanID == ''"

# Find trace IDs for a service
cx spans "filter \$l.serviceName == 'payment-service' | distinct \$d.traceID"
```

### Latency Analysis

```bash
# Spans slower than 1 second
cx spans 'filter $m.duration > 1000000'

# Top 10 slowest operations by average duration
cx spans 'groupby $l.operationName aggregate avg($m.duration) as avg_latency | orderby avg_latency desc | limit 10'

# Average latency by service
cx spans 'groupby $l.serviceName aggregate avg($m.duration) as avg_latency'

# P95 latency by operation
cx spans 'groupby $l.operationName aggregate percentile(0.95, $m.duration) as p95_latency'
```

### Latency Spike Detection

Find when latency changed over time:

```bash
# Average latency per 15-minute window
cx spans "filter \$l.serviceName == 'api' | groupby roundTime(\$m.timestamp, 15m) as interval aggregate avg(\$m.duration) as avg_latency | orderby interval"

# Find the time windows with highest latency
cx spans "filter \$l.serviceName == 'api' | groupby roundTime(\$m.timestamp, 5m) as interval aggregate avg(\$m.duration) as avg_latency | orderby avg_latency desc | limit 10"
```

### Error Investigation

```bash
# All error spans
cx spans 'filter $d.tags.error == true'

# Error spans for a specific service
cx spans "filter \$l.serviceName == 'checkout' | filter \$d.tags.error == true"

# Error rate by service
cx spans 'filter $d.tags.error == true | groupby $l.serviceName aggregate count() as errors | orderby errors desc'

# Error rate over time
cx spans 'filter $d.tags.error == true | groupby roundTime($m.timestamp, 15m) as interval aggregate count() as errors'
```

### Sampling Error Types

Find top error patterns with a sample span for each:

```bash
# Group errors by operation with a sample
cx spans 'filter $d.tags.error == true | groupby $l.operationName aggregate any_value($d) as sample, count() as total | orderby total desc | limit 5'

# Group by service and operation to see where errors concentrate
cx spans 'filter $d.tags.error == true | groupby $l.serviceName, $l.operationName aggregate count() as errors | orderby errors desc | limit 10'
```

### Finding Unique Values

Discover what's in the data before building complex queries:

```bash
# List all services with spans
cx spans 'distinct $l.serviceName'

# List all operations for a service
cx spans "filter \$l.serviceName == 'api' | distinct \$l.operationName"

# Find unique trace IDs for error spans
cx spans 'filter $d.tags.error == true | distinct $d.traceID'

# List applications and subsystems
cx spans 'distinct $l.applicationName'
cx spans 'distinct $l.subsystemName'
```

### Correlating by ID

```bash
# Find all spans for a request ID (field name varies by customer)
cx spans "filter \$d.request_id == 'abc-123-def'"

# Find spans for a specific user
cx spans "filter \$d.user_id == 'user_12345'" --start now-24h

# Find spans across services for the same trace
cx spans "filter \$d.traceID == 'abc123' | groupby \$l.serviceName aggregate count() as span_count, avg(\$m.duration) as avg_latency"
```

### Service Interaction Analysis

```bash
# Service and operation breakdown
cx spans 'groupby $l.serviceName, $l.operationName aggregate count() as span_count | orderby span_count desc'

# Which services talk to each other (via shared trace IDs)
cx spans "filter \$d.traceID == '<trace_id>' | groupby \$l.serviceName aggregate count() as span_count, avg(\$m.duration) as avg_latency"

# Busiest operations per service
cx spans 'groupby $l.serviceName, $l.operationName aggregate count() as calls, avg($m.duration) as avg_latency | orderby calls desc | limit 20'
```

### Time-Based Analysis

```bash
# Span count per hour
cx spans 'groupby roundTime($m.timestamp, 1h) as hour aggregate count() as span_count'

# Error rate over 15-minute intervals
cx spans 'filter $d.tags.error == true | groupby roundTime($m.timestamp, 15m) as interval aggregate count() as errors'
```

---

## Performance Tips

- Use `--limit` for exploratory queries
- Use `groupby` with aggregations instead of fetching raw spans when possible
- Filter by time first when dealing with large datasets
- Use specific filters (service name, operation) to reduce scan scope
- Don't rely solely on aggregations — retrieve sample spans to find information you didn't anticipate

## Debugging a Request

1. Get the trace ID from logs, headers, or error reports
2. Fetch all spans: `cx spans "filter \$d.traceID == '<id>'"`
3. Identify slow spans: look at `$m.duration`
4. Check for errors: look for `$d.tags.error == true`
5. Follow the call chain using `parentSpanID`
6. Correlate with logs: use the trace ID or timestamps to find related log entries

## Working with Large Result Sets

For large queries, use `--output agents` which automatically spills to a temp file when results exceed the configured threshold:

```bash
cx spans "filter \$l.serviceName == 'api'" --start now-24h --limit 1000 -o agents
```
