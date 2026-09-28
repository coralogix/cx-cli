# Multi-profile fan-out

A profile is one connection to Coralogix — a region or endpoint, its credentials,
and a default output format. Most people end up with several: one per team, one
per region, or one that returns `text` for reading by eye and one that returns
`toon` for an agent to consume.

Querying several teams is where the CLI earns its keep. In the Coralogix UI you
switch teams one at a time; through the MCP server it isn't really reachable at
all. Here it's one flag — or one command across every team at once.

## The default profile

`cx init` creates your first profile and makes it the default. **Every command you
run without `-p` uses that default**, so day-to-day work needs no flag at all:

```bash
cx logs 'source logs | limit 10'          # runs against the default profile
```

The default is recorded as `default_profile` in `~/.cx/config.toml`. To see which
profiles exist and which one is default, and to change it:

```bash
cx profiles list
cx profiles set-default prod-us
```

Add profiles as you need them and select one explicitly with `-p`:

```bash
cx profiles add prod-us
cx logs 'filter $m.severity == ERROR' -p prod-us
```

Naming a profile you haven't created is an error rather than a silent fallback to
the default:

```
Profile 'prod-us' not found. Run `cx profiles add` to set it up.
```

## Fanning out

`cx` can also run the same command across multiple profiles simultaneously — querying two teams at once is a single invocation. This is useful for querying across environments (prod + staging) or regions.

### Usage

Repeat the `-p` (or `--profile`) flag to target multiple profiles:

```bash
cx logs 'filter $m.severity == ERROR' -p prod -p staging
cx metrics query 'up' -p us-prod -p eu-prod
```

### How it works

1. Each profile is resolved into an independent execution target with its own API client.
2. The command runs concurrently against all targets.
3. Results are merged into a single output, with each row tagged with a `"profile"` key identifying its source.
4. Errors from individual profiles are printed to stderr but do not fail the entire operation - successful results are still returned. The command exits 0 if at least one profile succeeds.

When a command has a result limit, the limit applies independently to each selected profile. For example, `cx logs '<query>' -p prod -p staging --limit 100` can return up to 100 results from `prod` and up to 100 results from `staging`.

### Result tagging

When multiple profiles are used, each result row includes an additional `"profile"` field:

```json
{"profile": "prod", "timestamp": "...", "message": "..."}
{"profile": "staging", "timestamp": "...", "message": "..."}
```

When a single profile is used, no `"profile"` field is added.

Text output adds a `Profile` column to tables for REST commands (alerts, dashboards, metrics, rules, iam, notifications, etc.). DataPrime commands (`logs`, `spans`, `dataprime query`) prefix each rendered row with `[<profile>]`.

### Restrictions

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
