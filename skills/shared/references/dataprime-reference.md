# DataPrime Query Language Reference

DataPrime is the query language used to search and analyze logs, spans, and other observability data in Coralogix.

## Query Structure

A DataPrime query is a pipeline of commands separated by `|`. Each command transforms the output of the previous one:

```dataprime
source logs | filter $m.severity == ERROR | groupby $l.subsystemname aggregate count() as errors
```

Every query targets a **source** (`logs`, `spans`, etc.). When using `cx logs` or `cx spans query`, the source is injected automatically. When using `cx dataprime query`, you must include the `source` command explicitly.

## Data Prefixes

All fields are accessed through three namespaces:

| Prefix | Description | Examples |
|--------|-------------|----------|
| `$m` | Metadata (system-managed) | `$m.timestamp`, `$m.severity`, `$m.duration` |
| `$l` | Labels (indexed key-value pairs) | `$l.applicationname`, `$l.subsystemname`, `$l.serviceName` |
| `$d` | User data (application payload) | `$d.message`, `$d.user_id`, `$d.traceID` |

`$d` is the default prefix and can sometimes be omitted, but being explicit avoids ambiguity.

## Commands

### Filtering and Selection

| Command | Description | Example |
|---------|-------------|---------|
| `filter` | Keep rows matching a condition | `filter $m.severity == ERROR` |
| `choose` | Select specific fields | `choose $m.timestamp, $d.message` |
| `limit` | Cap the number of results | `limit 10` |
| `wildfind` | Search all fields for a string | `wildfind "connection refused"` |

### Aggregation

| Command | Description | Example |
|---------|-------------|---------|
| `groupby` | Group rows and apply aggregations | `groupby $l.subsystemname aggregate count() as n` |
| `multigroupby` | Group by multiple field sets | `multigroupby a, b aggregate count()` |
| `count` | Count all rows | `count` |
| `countby` | Count rows grouped by a field | `countby $l.applicationname` |
| `distinct` | Return unique values of a field | `distinct $l.subsystemname` |

### Transformation

| Command | Description | Example |
|---------|-------------|---------|
| `create` | Add a computed field | `create latency_ms from $m.duration / 1000` |
| `orderby` | Sort results | `orderby $d.timestamp desc` |
| `extract` | Parse fields with regex or JSON | See [Text Extraction](#text-extraction) |
| `dedupeby` | Remove duplicates by a field | `dedupeby $m.templateid` |

## Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `==` | Equals | `filter $m.severity == ERROR` |
| `!=` | Not equals | `filter $l.subsystemname != 'test'` |
| `>`, `<`, `>=`, `<=` | Comparison | `filter $d.response_time > 1000` |
| `~` | Contains (substring match) | `filter $d.message ~ 'timeout'` |
| `&&` | AND | `filter $m.severity == ERROR && $l.applicationname == 'api'` |
| `\|\|` | OR | `filter $m.severity == ERROR \|\| $m.severity == CRITICAL` |

## Type Conversions

Cast fields inline with `:type`:

```dataprime
filter $d.http_error_code:number == 500
```

Supported types: `bool`, `number`, `string`, `timestamp`, `interval`, `array`, `object`

## Field Access

```dataprime
# Chained field names (dot notation)
filter $d.tags.user_context.email == 'test@example.com'

# Special characters require brackets
filter $d.http['status/code'] == 500
```

## Aggregation Functions

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

## Time-Based Grouping

Use `roundTime()` to bucket timestamps:

```bash
# Group by hour
cx logs 'groupby roundTime($m.timestamp, 1h) as hour aggregate count() as count'

# Error rate over 15-minute intervals
cx logs 'filter $m.severity == ERROR | groupby roundTime($m.timestamp, 15m) as interval aggregate count() as errors'
```

## Multi-Value Matching

Use `arrayContains` to match against a set of values:

```bash
# Match multiple subsystems
cx logs 'filter ["api", "web", "worker"].arrayContains($l.subsystemname)'

# Match multiple severity levels
cx logs 'filter [ERROR, CRITICAL].arrayContains($m.severity)'
```

## Text Extraction

### Regex Extraction

```bash
# Extract with unnamed capture group
cx logs 'extract $d.email into domain using regexp(e=/@(.*)/) | distinct $d.domain._0'

# Named capture groups
cx logs 'extract $d.email into extracted using regexp(e=/(?<username>[a-zA-Z0-9._%+-]+)@(?<domain>.*)/) | choose $d.extracted.username, $d.extracted.domain'
```

### JSON String Parsing

```bash
# Parse a JSON string field into an object for further querying
cx logs 'extract $d.json_payload into parsed using jsonobject() | filter $d.parsed.status == "failed"'
```

## Deduplication

```bash
# Remove duplicates by log template
cx logs 'dedupeby $m.templateid'

# Dedupe by a custom field
cx logs 'dedupeby $d.request_id'
```

## Built-In Documentation

For the full list of commands and functions with detailed syntax:

```bash
cx dataprime list                              # List all commands and functions
cx dataprime list --filter commands             # Commands only
cx dataprime list --filter functions --name time # Search functions by name
cx dataprime show filter                        # Detailed help for a specific command
cx dataprime show groupby
```
