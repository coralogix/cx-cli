# Multi-profile fan-out

`cx` can run the same command across multiple Coralogix profiles simultaneously. This is useful for querying across environments (prod + staging) or regions in a single invocation.

## Usage

Repeat the `-p` (or `--profile`) flag to target multiple profiles:

```bash
cx logs 'filter $m.severity == ERROR' -p prod -p staging
cx metrics query 'up' -p us-prod -p eu-prod
```

## How it works

1. Each profile is resolved into an independent execution target with its own API client.
2. The command runs concurrently against all targets.
3. Results are merged into a single output, with each row tagged with a `"profile"` key identifying its source.
4. Errors from individual profiles are printed to stderr but do not fail the entire operation - successful results are still returned. The command exits 0 if at least one profile succeeds.

When a command has a result limit, the limit applies independently to each selected profile. For example, `cx incidents list -p prod -p staging --limit 100` can return up to 100 incidents from `prod` and up to 100 incidents from `staging`.

## Result tagging

When multiple profiles are used, each result row includes an additional `"profile"` field:

```json
{"profile": "prod", "timestamp": "...", "message": "..."}
{"profile": "staging", "timestamp": "...", "message": "..."}
```

When a single profile is used, no `"profile"` field is added.

Text output adds a `Profile` column to tables for REST commands (alerts, dashboards, metrics, rules, iam, notifications, etc.). DataPrime commands (`logs`, `spans`, `dataprime query`) prefix each rendered row with `[<profile>]`.

## Restrictions

`--api-key` and `--region` overrides are incompatible with multiple `-p` flags. These overrides apply to a single profile and would be ambiguous when targeting multiple profiles:

```bash
# This will error
cx logs 'filter $m.severity == ERROR' -p prod -p staging --api-key sk-...
```

Instead, store per-profile credentials ahead of time:

```bash
cx profiles add prod
cx profiles add staging
```
