# Time Syntax

Commands that accept `--start` and `--end` flags (`logs`, `metrics query-range`, `traces search`, `traces get`) support the following time expressions.

## Formats

| Format | Example | Description |
|--------|---------|-------------|
| `now` | `now` | Current UTC time |
| Relative | `now-1h` | Subtract a duration from now |
| Relative (spaces) | `now - 3d` | Spaces around `-` are allowed |
| ISO 8601 / RFC 3339 | `2024-01-01T00:00:00Z` | Absolute UTC timestamp |

## Duration Tokens

Relative expressions use [humantime](https://docs.rs/humantime) duration syntax:

| Token | Meaning |
|-------|---------|
| `s` | Seconds |
| `m` | Minutes |
| `h` | Hours |
| `d` | Days |
| `w` | Weeks |

Compound durations are supported: `1h30m`, `2d12h`.

## Defaults by Command

| Command | `--start` default | `--end` default |
|---------|-------------------|-----------------|
| `cx logs` | `now-1h` | `now` |
| `cx metrics query-range` | `now-1h` | `now` |
| `cx traces search` | `now-1h` | `now` |
| `cx traces get` | `now-1h` | `now` |
| `cx metrics query` | Uses `--time` (defaults to now) | N/A |

## Examples

```bash
# Last hour (default)
cx logs 'source logs'

# Last 6 hours
cx logs 'source logs' --start now-6h

# Specific time window
cx logs 'source logs' --start 2024-01-01T00:00:00Z --end 2024-01-01T01:00:00Z

# Last 3 days of traces
cx traces search my-service --start now-3d
```
