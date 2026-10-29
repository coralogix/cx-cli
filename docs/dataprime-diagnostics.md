# DataPrime validation and diagnostics

Applies to `cx logs`, `cx spans`, and `cx dataprime query`.

## Strict field validation

Strict field validation is off by default, the same as the public API. A query
that references a field missing from the schema still runs. The API sends a
`compileWarning` with `keypath does not exist`, and the CLI prints it on stderr.

Use `--strict-fields-validation` (or `=true`) to reject these queries instead.
The error ends with a hint pointing at `--strict-fields-validation=false` and at
`strict_fields_validation` in `~/.cx/config.toml`.

Strict validation can reject fields that exist:

- New fields that are not in the schema yet (ingestion delay).
- Fields the schema dropped because a JSON branch has too many children.
- All fields, including standard `$m`/`$l` fields, when the time range has no
  data. Without strict validation, the API sends an
  `archiveWarning.noMetastoreData` warning for this case.

To turn strict validation on by default for all profiles, set it in
`~/.cx/config.toml`:

```toml
strict_fields_validation = true
```

Put the line above any `[section]` or `[[section]]` header, such as
`[[managed_completions]]`. A line placed after a section header belongs to that
section and is silently ignored.

The flag always wins over the config setting.

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
cx spans 'limit 10' --strict-fields-validation --extended-information -o json
cx dataprime query 'source logs | limit 10' --extended-information -o json
```

For one profile, the extended response is `{ "results": [...], "queryId": "...",
"warnings": [...], "statistics": {...} }`. Missing query IDs/statistics are
`null`; absent warnings are `[]`. JSON results keep their existing normalization.
Non-aggregated TOON result rows are renamed to `$m`, `$l`, and `$d` first, the
same as a normal `toon` query.

For multiple profiles, the response is `{ "results": [...], "queries": [...] }`.
Each query entry contains `profile`, `queryId`, `warnings`, and `statistics`;
result rows retain their profile tags. IDs and statistics are not combined.

Text mode keeps the existing result rendering and appends a JSON diagnostics
object after a blank line. For one profile, this object contains `queryId`,
`warnings`, and `statistics`; for multiple profiles it contains `queries`.
JSON mode prints the combined envelope. TOON mode encodes that envelope as TOON.
A non-aggregated TOON payload larger than `max_dataprime_direct_output_size` is
written to the temp file as JSON, and stdout is the result count plus that path.
The file is the full envelope, not the rows alone. Aggregated TOON output is not
spilled. JSON and text extended output stay on stdout.
Without the flag, the existing stdout format and spilling behavior are preserved.
Query failures still return a nonzero exit code.
