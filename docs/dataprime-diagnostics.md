# DataPrime validation and diagnostics

Applies to `cx logs`, `cx spans`, and `cx dataprime query`.

## Strict field validation

Queries that reference fields missing from the schema are rejected by default. Use `--strict-fields-validation=false` to allow unknown fields with
compile warnings. `--strict-fields-validation` (or `=true`) enables validation
explicitly. This CLI default differs from the public API's permissive default.

When a query fails with `keypath does not exist`, the error ends with a hint
pointing at `--strict-fields-validation=false`. Use it when the field exists in
the data but not in the schema yet (ingestion delay, or the schema dropped it
because a JSON branch has too many children).

To make permissive validation the default for a profile, set it in
`~/.cx/profiles/<name>.toml`:

```toml
strict_fields_validation = false
```

Put the line at the top of the file, above any `[section]` header such as
`[oauth_tokens]`. A line placed after a section header belongs to that section
and is silently ignored.

The flag always wins over the profile setting. With multiple profiles, each
profile uses its own setting unless the flag is passed.

## Warnings

All API warnings are printed to stderr as complete JSON envelopes, including
limit warnings, archive warnings, and warnings preceding a failed query. Each
warning is one JSON line, for example:

```json
{"warning":{"numberOfResultsLimitWarning":{"numberOfResultsLimit":2000}},"profile":"prod"}
```

Read stderr as well as stdout: a warning can mean the results are incomplete
(result limit hit, archive data missing, bytes-scanned limit reached).

## Extended information

Add `--extended-information` to include query information on stdout:

```bash
cx logs 'limit 10' --extended-information -o json
cx spans 'limit 10' --strict-fields-validation=false --extended-information -o json
cx dataprime query 'source logs | limit 10' --extended-information -o json
```

For one profile, the extended JSON/TOON response is `{ "results": [...], "queryId": "...",
"warnings": [...], "statistics": {...} }`. Missing query IDs/statistics are
`null`; absent warnings are `[]`. Results keep their existing normalization.

For multiple profiles, the response is `{ "results": [...], "queries": [...] }`.
Each query entry contains `profile`, `queryId`, `warnings`, and `statistics`;
result rows retain their profile tags. IDs and statistics are not combined.

Text mode keeps the existing result rendering and appends a JSON diagnostics
object after a blank line. For one profile, this object contains `queryId`,
`warnings`, and `statistics`; for multiple profiles it contains `queries`.
JSON mode returns the combined envelope; TOON mode encodes that envelope as TOON.
Extended output stays on stdout, bypassing result spilling.
Without the flag, the existing stdout format and spilling behavior are preserved.
Query failures still return a nonzero exit code.
