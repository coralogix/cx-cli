# Logs - Advanced Usage

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

### 3. Discover Fields When Needed

For customer-specific data:

```bash
cx search-fields "customer identifier" --dataset logs
cx search-fields "error message" --dataset logs
```

### 4. Build and Execute Query

Start simple, add complexity:

```bash
# Step 1: Check if data exists
cx logs "filter \$l.subsystemname == 'checkout'" --limit 10

# Step 2: Add filters
cx logs "filter \$l.subsystemname == 'checkout' && \$m.severity == ERROR"

# Step 3: Add aggregation
cx logs "filter \$l.subsystemname == 'checkout' && \$m.severity == ERROR | groupby \$d.error_type aggregate count() as occurrences"
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

### Correlating by ID

```bash
# Find all logs for a request ID
cx logs "filter \$d.request_id == 'abc-123-def'"

# Find logs for a user
cx logs "filter \$d.user_id == 'user_12345'" --start now-24h
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

## Debugging a Production Issue

1. Start with recent errors: `cx logs 'filter $m.severity == ERROR' --limit 20`
2. Identify the affected service from log labels
3. Narrow to that service: `cx logs "filter \$l.subsystemname == 'service-name' && \$m.severity == ERROR"`
4. Search for patterns in error messages
5. Correlate with request IDs or user IDs
6. Check time patterns for spikes

## Working with Large Result Sets

For large queries, use `--output agents` which automatically spills to a temp file when results exceed the configured threshold:

```bash
cx logs 'filter $m.severity == ERROR' --start now-24h --limit 1000 -o agents
```
