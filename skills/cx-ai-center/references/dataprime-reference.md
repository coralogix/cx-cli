# DataPrime Query Language Reference

## Query Structure

A DataPrime query is a pipeline of commands separated by `|`. Each command transforms the output of the previous one:

```dataprime
filter $m.severity == ERROR | groupby $l.subsystemname aggregate count() as errors
```

### Source Handling

Every query targets a **source** (`logs`, `spans`, etc.). The source is set by whichever `cx` command you use. A full query with an explicit source looks like:

```dataprime
source <logs|spans> | filter ... | groupby ...
```

When running via a source-specific command (e.g. `cx logs`, `cx spans`), the source is injected automatically - omit it from the query. When running via `cx dataprime query`, use the `--source` flag or include `source` in the query itself.

The examples below focus on the DataPrime query language and omit the source and CLI command prefix.

### Comments

Comments are supported with `#` or `//`:

```dataprime
filter $m.severity == ERROR  # only errors
| limit 10                   // cap results
```

## Data Prefixes

All fields are accessed through three namespaces:

| Prefix | Description | Examples |
|--------|-------------|----------|
| `$m` | Metadata (system-managed) | `$m.timestamp`, `$m.severity`, `$m.duration` |
| `$l` | Labels (indexed key-value pairs) | `$l.applicationname`, `$l.subsystemname`, `$l.serviceName` |
| `$d` | User data (application payload) | `$d.message`, `$d.user_id`, `$d.traceID` |

`$d` is the default prefix and can sometimes be omitted, but being explicit avoids ambiguity.

## Data Types

| Type | Description | Example |
|------|-------------|---------|
| `string` | Text, enclosed in **single quotes** | `'some_text'` |
| `number` | Numeric value | `123`, `3.14` |
| `boolean` | True or false | `true`, `false` |
| `timestamp` | Date and time (nanoseconds since epoch) | `1714636800000000000` |
| `interval` | Time duration | `1h`, `1d`, `1w` |
| `array` | List of values | `[1, 2, 3]` |
| `object` | Key-value pairs | `{"name": "John"}` |
| `null` | Missing value or key | `null` |

## Commands

### Filtering and Selection

| Command | Description | Example |
|---------|-------------|---------|
| `filter` | Keep rows matching a condition | `filter $m.severity == ERROR` |
| `choose` | Select specific fields | `choose $m.timestamp, $d.message` |
| `limit` | Cap the number of results | `limit 10` |
| `wildfind` | Token match across the whole record (see note below) | `wildfind 'connection refused'` |
| `lucene` | Filter using Lucene syntax (`field:value`, field names relative to `$d`, combine with `AND`/`OR`/parens) | `lucene 'field:"value"'` |

> **Note on `wildfind`:** It is a standalone command, not a condition within `filter`. You cannot combine it with other filter expressions - use it as its own pipeline stage.

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
| `dedupeby` | Remove duplicates by a field (cost grows with number of distinct keys) | `dedupeby $m.templateid` |

## Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `==` | Equals | `filter $m.severity == ERROR` |
| `!=` | Not equals | `filter $l.subsystemname != 'test'` |
| `>`, `<`, `>=`, `<=` | Comparison | `filter $d.response_time > 1000` |
| `~` | Case-insensitive token match (matches whole tokens, not arbitrary substrings) | `filter $d.message ~ 'timeout'` |
| `&&` | AND | `filter $m.severity == ERROR && $l.applicationname == 'api'` |
| `\|\|` | OR | `filter $m.severity == ERROR \|\| $m.severity == CRITICAL` |
| `!= null` | Field exists | `filter $d.some_field != null` |

> **`~` vs `contains()`:** `~` is a **case-insensitive, token-based** match — it matches whole tokens (words), so `~ 'timeout'` also matches `TIMEOUT` and `Timeout`. `contains()` is a **case-sensitive raw substring** match — `contains('time')` matches inside `timeout`, but only in that exact case. Use `~` for word/term search; use `contains()` when you need an exact-case partial-string match.

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
| `median($field)` | Median value |
| `stddev($field)` | Standard deviation |
| `variance($field)` | Variance |
| `distinct_count($field)` | Count unique values (exact) |
| `approx_count_distinct($field)` | Approximate count of unique values |
| `any_value($field)` | Random sample value |
| `collect($field)` | Collect values into an array |

> **`distinct_count` vs `approx_count_distinct`:** Exact `distinct_count` can be slow or run out of memory on high-cardinality fields. When an exact count isn't required, use `approx_count_distinct` instead.

Example - full CLI invocation:
```bash
cx dataprime query --source logs 'groupby $l.subsystemname aggregate count() as error_count, avg($d.response_time) as avg_response | orderby error_count desc'
```

## Utility Functions

### firstNonNull - Field Coalescing

Return the first non-null value from a list of fields. Useful when the same data may appear in different fields across log sources:

```dataprime
# Merge fields
create message from firstNonNull($d.error_message, $d.msg, $d.body)

# Use inside groupby
groupby firstNonNull($d.error_message, $d.msg) as message aggregate count() as n
```

### Template Sampling

Find top error patterns with a sample message for each:

```dataprime
filter $m.severity == ERROR | groupby $m.templateid aggregate any_value($d) as sample, count() as total | orderby total desc | limit 5
```

## Time-Based Grouping

Use `roundTime()` to bucket timestamps:

```dataprime
# Group by hour
groupby roundTime($m.timestamp, 1h) as hour aggregate count() as count

# Error rate over 15-minute intervals
filter $m.severity == ERROR | groupby roundTime($m.timestamp, 15m) as interval aggregate count() as errors
```

## Multi-Value Matching

Use `arrayContains` to match against a set of values:

```dataprime
# Match multiple subsystems
filter ['api', 'web', 'worker'].arrayContains($l.subsystemname)

# Match multiple severity levels
filter [ERROR, CRITICAL].arrayContains($m.severity)
```

## Text Extraction

### Regex Extraction

```dataprime
# Extract with unnamed capture group
extract $d.email into domain using regexp(e=/@(.*)/) | distinct $d.domain._0

# Named capture groups
extract $d.email into extracted using regexp(e=/(?<username>[a-zA-Z0-9._%+-]+)@(?<domain>.*)/) | choose $d.extracted.username, $d.extracted.domain
```

### JSON String Parsing

```dataprime
# Parse a JSON string field into an object for further querying
extract $d.json_payload into parsed using jsonobject() | filter $d.parsed.status == 'failed'
```

## Deduplication

```dataprime
# Remove duplicates by log template
dedupeby $m.templateid

# Keep one row per key (cost grows with the number of distinct keys)
dedupeby $d.session_id
```

> **Performance:** `dedupeby` cost scales with the number of **distinct keys** it tracks, so it gets slow and memory-heavy on high-cardinality or near-unique keys (e.g. a request id). Cardinality depends on your data and time window — a key like session or trace id can still be large. Keep `dedupeby` when you genuinely need one representative row per key; to cut cost, narrow the input first (tighter `filter`, smaller time window) rather than swapping in `filter`/`limit` (which does **not** keep one row per key) or an aggregation (which changes the row shape) unless that different output is acceptable. The `dedupeby <key> orderby ...` form (keep the latest row per key) is heavier still.

## Built-In Documentation

For the full list of commands and functions with detailed syntax:

```bash
cx dataprime list                              # List all commands and functions
cx dataprime list --filter commands             # Commands only
cx dataprime list --filter functions --name time # Search functions by name
cx dataprime show filter                        # Detailed help for a specific command
cx dataprime show groupby
```

## Validating a DataPrime query

A query that looks right can still fail on a typoed field path, an invented function, or a malformed pipeline stage. Validate before trusting the output — a short-window run through the CLI is cheap and catches almost all of these:

```bash
cx logs  '<pipeline>' --start now-15m --end now --limit 1
cx spans '<pipeline>' --start now-15m --end now --limit 1
```

`now-15m` is a good default; widen it only if 15 minutes is unlikely to exercise the pipeline. Per "Source Handling" above, omit any leading `source logs` / `source spans` — `cx logs` and `cx spans` inject the source themselves.

Check both the exit code and the output — some errors surface only in the output.

**Pass** = exit 0 and the output is rows or `[]` with no error or warning lines.

**Hard fail** — query is broken, fix it:
- non-zero exit
- `error from profile '...': API request failed` — HTTP error from the API
- `Compilation errors:` — parse error, unknown function, malformed expression

**Soft fail** (needs investigation):
- `keypath does not exist` — the query parsed, but no record in the window had the referenced field. This is ambiguous: the field name might be a typo, or it might be real but absent from records in this 15-minute slice. Confirm with `cx search-fields "<field hint>" --dataset logs` (or `--dataset spans`). If the field is real, the query is fine — try a wider window or accept the empty result. If it isn't, fix the field name.

On fail: re-discover fields with `cx search-fields`, look up command syntax with `cx dataprime show <command>`, fix, re-run.
