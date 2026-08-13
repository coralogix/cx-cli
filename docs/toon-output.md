# TOON output format

The `toon` output mode (`-o toon`) produces token-optimized output designed for AI agent consumption. It reduces token usage through key renaming, metadata stripping, and large-result spilling.

> **Note:** `agents` is accepted as a deprecated alias for `toon` — `-o agents`, `CX_*` config values, and `default_output_format = "agents"` all still work and resolve to the same mode. Prefer `toon` in new usage.

All `toon` output is TOON-encoded via `toon_format::encode_default()` for compact serialization.

## Key transformations

For DataPrime results (logs and spans), the following keys are renamed:

| Original key | TOON key | Description |
|---|---|---|
| `metadata` | `$m` | Log metadata (severity, timestamp, etc.) |
| `labels` | `$l` | Application labels (`applicationname`, `subsystemname`, `serviceName`, etc.) |
| `userData` | `$d` | User data / log body |

The aliases `$m`, `$l`, and `$d` are also valid in DataPrime query syntax itself - for example, `cx logs 'filter $m.severity == ERROR'` works regardless of output mode.

## Metadata stripping

The following metadata fields are removed from `$m`:

- `branchid`
- `priorityclass`
- `processingOutputTimestampMicros`
- `processingOutputTimestampNanos`
- `timestampMicros`

## Metrics output

For `cx metrics query` (instant), toon output includes only the metric definition (labels) and sample value — timestamps are omitted since instant queries return a single point in time.

For `cx metrics query-range`, toon output produces **one row per time-series**. Each row contains the metric labels as individual columns, followed by one column per data point whose header is the ISO-8601 timestamp (e.g. `2026-05-14T12:00:00Z`) and whose value is the sample value. This avoids repeating long label strings across every data point. When querying multiple profiles, a `profile` column is included between the label columns and the timestamp columns. Epoch timestamps from the API are automatically converted to ISO-8601.

## Result spilling

When a non-aggregated DataPrime result set in `toon` mode exceeds `max_dataprime_direct_output_size` (default `102400` bytes, or 100 KiB), the full results are written to a temp file instead of stdout.

What gets printed to stdout:

```
<N> results written to /tmp/cx_results_<hash>.json
```

What gets written to the file: the full JSON array of transformed results.

### Config keys

| Key | Default | Description |
|---|---|---|
| `max_dataprime_direct_output_size` | `102400` (100 KiB) | Byte threshold for spilling. Set to `-1` to disable |
| `temp_dir` | `"/tmp/"` | Directory for spilled files |

### File naming

Spilled files follow the pattern `cx_results_<hash>.json`, where `<hash>` is an 8-character hex string derived from the file contents.

### Cleanup

Spilled files older than 30 minutes can be removed with:

```bash
cx cleanup
```

This deletes all `cx_results*` files in `temp_dir` that are older than 30 minutes.

## Command discovery with `cx schema`

`cx schema` outputs the full command tree as JSON, including all commands, subcommands, arguments, flags, and descriptions. Agents should use this to discover available commands programmatically rather than parsing help text.

```bash
cx schema
```

The output is a JSON object describing the entire CLI hierarchy. This is useful for:
- Discovering which commands and subcommands exist
- Learning the required and optional arguments for each command
- Building dynamic tool interfaces around `cx`

## AI agent integration

AI agents consuming `cx` output should:

1. Run `cx schema` to discover available commands and their flags.
2. Use `-o toon` for all queries (`-o agents` is a deprecated alias).
3. Check whether the output is a file-path reference (spilled result) and read the file if so.
4. Use `cx cleanup` periodically to remove stale result files.
5. Reference fields using `$d`, `$l`, and `$m` notation in follow-up DataPrime queries.
6. Pass `--yes` for destructive operations (create, update, delete, etc.) under `iam` and `archive` commands. Without `--yes`, these commands require interactive confirmation and will fail on non-interactive terminals with: `"This operation requires confirmation but stdin is not a terminal. Pass --yes to skip the confirmation prompt."` Subcommands tagged `[requires --yes]` in their help text are the ones that need it. Always confirm with the user before running destructive operations.
