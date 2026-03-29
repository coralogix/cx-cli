# Agents Output Format

The `agents` output mode (`-o agents`) produces token-optimized JSON designed for AI agent consumption. It reduces token usage through key renaming, metadata stripping, and large-result spilling.

## Key Transformations

For DataPrime results (logs and traces), the following keys are renamed:

| Original Key | Agents Key | Description |
|-------------|------------|-------------|
| `metadata` | `$m` | Log metadata (severity, timestamp, etc.) |
| `labels` | `$l` | Application labels (applicationname, subsystemname, etc.) |
| `userData` | `$d` | User data / log body |

## Metadata Stripping

The following metadata fields are removed from `$m` as they carry no signal for AI agents:

- `branchid`
- `priorityclass`
- `processingOutputTimestampMicros`
- `processingOutputTimestampNanos`
- `timestampMicros`

## Metrics Output

For `cx metrics query`, agents output includes only the metric definition (labels) and sample value — timestamps are omitted.

## Result Spilling

When a non-aggregated DataPrime result set in `agents` mode exceeds `max_dataprime_direct_output_size` (default: 100 KiB), the full results are written to a temp file instead of stdout.

**What gets printed to stdout:**

```
<N> results written to /tmp/cx_results_<hash>.json
```

**What gets written to the file:** The full JSON array of transformed results.

### Config Keys

| Key | Default | Description |
|-----|---------|-------------|
| `max_dataprime_direct_output_size` | `102400` | Byte threshold for spilling. Set to `-1` to disable |
| `temp_dir` | `"/tmp/"` | Directory for spilled files |

### File Naming

Spilled files follow the pattern `cx_results_<hash>.json` where `<hash>` is an 8-character hex string derived from the file contents.

### Cleanup

Spilled files older than 30 minutes can be removed with:

```bash
cx cleanup
```

This deletes all `cx_results*` files in `temp_dir` that are older than 30 minutes.

## AI Agent Integration

AI agents consuming `cx` output should:

1. Use `-o agents` for all queries
2. Check if the output is a file path reference (spilled result) and read the file if so
3. Use `cx cleanup` periodically to remove stale result files
4. Reference fields using `$d`, `$l`, `$m` notation in follow-up DataPrime queries
