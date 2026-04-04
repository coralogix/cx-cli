# Multi-Profile Fan-Out

`cx` can run the same command across multiple Coralogix profiles simultaneously. This is useful for querying across environments (e.g. prod + staging) or regions in a single invocation.

## Usage

Repeat the `--profile` (or `-p`) flag to target multiple profiles:

```bash
cx logs 'filter $d.severity == "ERROR"' -p prod -p staging
cx metrics query 'up' -p us-prod -p eu-prod
```

## How It Works

1. Each profile is resolved into an independent execution target with its own API client
2. The command runs concurrently against all targets
3. Results are merged into a single output, with each row tagged with a `"profile"` key identifying its source
4. Errors from individual profiles are printed to stderr but do not fail the entire operation — successful results are still returned

## Result Tagging

When multiple profiles are used, each result row includes an additional `"profile"` field:

```json
{"profile": "prod", "timestamp": "...", "message": "..."}
{"profile": "staging", "timestamp": "...", "message": "..."}
```

When only a single profile is used, no `"profile"` tag is added.

## Restrictions

`--api-key` and `--region` overrides are **incompatible** with multiple `--profile` flags. These overrides apply to a single profile and would be ambiguous when targeting multiple profiles.

```bash
# This will error:
cx logs 'query' -p prod -p staging --api-key sk-...

# Instead, store per-profile credentials:
cx profiles add prod
cx profiles add staging
```
