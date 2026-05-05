# Metrics Querying Reference

Query and analyze Coralogix metrics using the `cx metrics` CLI commands with PromQL.

> **PromQL syntax:** See `promql-guidelines.md` for the full query language reference.

## CLI Commands

All metrics operations use `cx metrics` with four subcommands:

| Command | Purpose | Key flags |
|---|---|---|
| `cx metrics search --name <pattern>` | Find metrics by name (wildcard or substring) | `--name` |
| `cx metrics get-labels <metric>` | List available label names for a metric | - |
| `cx metrics query '<expr>'` | Instant PromQL query (single point in time) | `--time <timestamp>` |
| `cx metrics query-range '<expr>'` | Range PromQL query (time series) | `--start`, `--end`, `--step` |

**Output format:** append `-o json` or `-o agents` to any command for machine-readable output.

### Search Examples

```bash
# Exact substring match
cx metrics search --name http_requests

# Wildcard: find all CPU metrics
cx metrics search --name '*cpu*'

# List all metrics
cx metrics search --name '*'
```

### Instant Query Examples

```bash
# Current state
cx metrics query 'up'

# At a specific time
cx metrics query 'rate(http_requests_total[5m])' --time 2024-01-01T12:00:00Z

# With output for further processing
cx metrics query 'sum by (service) (rate(http_errors_total[5m]))' -o agents
```

### Range Query Examples

```bash
# Last hour, default step (1m)
cx metrics query-range 'rate(http_requests_total[5m])'

# Custom window and step
cx metrics query-range 'sum by (service) (rate(http_requests_total[5m]))' \
  --start now-6h --end now --step 5m

# Daily aggregation over the last week
cx metrics query-range 'max by () (max_over_time(cpu_usage[1d]))' \
  --start now-7d --end now --step 1d
```

### Label Discovery Example

```bash
cx metrics get-labels http_requests_total
# Returns: job, instance, method, route, status_code, ...
```

## Time Syntax

All time arguments accept:
- Relative: `now`, `now-1h`, `now-30m`, `now-2d`, `now-1w`
- Absolute: RFC3339/ISO 8601 - `2024-01-01T00:00:00Z`

---

## Investigation Workflow

### 1. Initial Assessment

When given a vague problem, ask 1–2 focused clarifying questions before proceeding:
- What exactly is failing or behaving unexpectedly?
- When did it start? What is the affected time window?

Prefer to start investigating immediately if the question is specific enough.

### 2. Metric Discovery

Always start by searching for relevant metrics before querying:

```bash
# Try domain-specific patterns first
cx metrics search --name '*http*'
cx metrics search --name '*error*'
cx metrics search --name '*latency*'
cx metrics search --name '*cpu*'
cx metrics search --name '*memory*'

# If nothing found, broaden the search
cx metrics search --name '*request*'
cx metrics search --name '*'   # full list as last resort
```

When two similar metrics are found and one is suffixed with `_count`, prefer the one without the suffix - `_count` typically tracks the number of observations, not the measured value itself.

### 3. Label Discovery

Once a relevant metric is identified, discover its labels before filtering:

```bash
cx metrics get-labels <metric_name>
```

Use the returned label names to build precise PromQL filters. Note: label *values* are not directly queryable via the CLI - infer them from query results or domain knowledge.

### 4. Query Construction & Execution

Choose the right query type:

- **Instant query** (`cx metrics query`) - use for current state, single values, or absolute aggregations over a window. Use `--time` to query historical data at a specific moment.
- **Range query** (`cx metrics query-range`) - use when comparing across time periods (e.g., per-day DAU, hourly error rate trend). Set `--step` to match any `[window]` used in temporal functions.

Start simple, add complexity as needed:

```bash
# Step 1: Check if metric exists and has data
cx metrics query 'http_requests_total'

# Step 2: Add label filters and aggregation
cx metrics query 'sum by (status) (rate(http_requests_total[5m]))'

# Step 3: Build the final diagnostic query
cx metrics query 'sum by (service) (rate(http_requests_total{status=~"5.."}[5m]))'
```

### 5. Retry Logic

If a query returns no results or an error:
1. Check metric name - run `cx metrics search --name '*<keyword>*'` with a broader term
2. Check label names - run `cx metrics get-labels <metric>` to verify filter keys
3. Widen the time range or shorten the rate window
4. If filtering on a label that may be empty, exclude empty values: `{label!=""}`
5. Try an alternative metric name or structure

Maximum 5 retry attempts per query, each with a concrete improvement.

### 6. Pattern Recognition & Root Cause Analysis

After collecting results:
- Correlate across metrics (e.g., error spike matches CPU spike?)
- Look for temporal patterns - recurring peaks, sudden step changes
- Cross-layer analysis: app → services → infrastructure → dependencies
- Provide actionable next steps, not just data

### 7. Summarize Frequently

PromQL results can be large. After every few queries, summarize:
- Key findings so far
- Queries already run
- Next planned queries
- Ask to continue if more investigation is needed

---

## Common Investigation Patterns

### HTTP Errors
1. Check error rate: `sum by (service) (rate(http_requests_total{status=~"5.."}[5m]))`
2. Compare to total RPS: `sum by (service) (rate(http_requests_total[5m]))`
3. Check pod/deployment health metrics
4. Check dependency latency

### Performance / Latency
1. Check p95/p99 latency via histograms: `histogram_quantile(0.95, sum by (le, service) (rate(http_request_duration_seconds_bucket[5m])))`
2. Check resource saturation: CPU, memory, disk
3. Check autoscaling metrics
4. Check dependency response times

### Availability
1. Check `up` metric across services: `cx metrics query 'up'`
2. Check pod restart counts
3. Check node health
4. Check service discovery metrics

---

## Key Principles

- **Discover before querying**: always search for metric names first
- **Instant over range**: prefer instant queries unless the question requires a time series
- **Align step with window**: when using `max_over_time(metric[1d])`, set `--step 1d`
- **Filter empty labels**: if results have blank label values, add `{label!=""}` to the filter
- **Aggregate early**: use `sum by (...)` to reduce cardinality before further operations
