---
name: cx-search-dashboard
description: This skill should be used when the user asks to "find a dashboard", "search dashboards", "does a dashboard exist for X", "find widgets that query Y", "which dashboards use this field", "find a dashboard about errors", "look up dashboards by description", "search for existing monitoring dashboards", "find widgets that reference a field", or wants to discover existing Coralogix dashboards or widgets using natural-language or field-based search.
metadata:
  version: "0.1.0"
---

# Dashboard Search Skill

Use this skill to discover existing Coralogix dashboards and widgets using semantic or field-based search via the `cx dashboards` CLI commands. Before creating a new dashboard, always search first to avoid duplication.

## CLI Commands

| Command | Purpose | Key flags |
|---|---|---|
| `cx dashboards search "<description>"` | Find dashboards by natural-language description | `--limit` |
| `cx dashboards query-search --description "<text>"` | Find widgets whose queries match a description | `--limit` |
| `cx dashboards query-search --field "<field-path>"` | Find all widgets that reference a specific field | `--limit` |
| `cx dashboards catalog -o json` | List all dashboards | - |
| `cx dashboards get <id> -o json` | Get full dashboard definition | - |

**Output format:** append `-o json` or `-o agents` for machine-readable output.

## When to Use Each Command

| Goal | Command |
|---|---|
| Check if a dashboard for a service or topic already exists | `cx dashboards search` |
| Find widgets whose queries cover a topic you care about | `cx dashboards query-search --description` |
| Find widgets whose queries reference a specific field path | `cx dashboards query-search --field` |
| Browse all dashboards | `cx dashboards catalog` |

## Examples

### Find a dashboard for a service

```bash
cx dashboards search "payment service error rate"
cx dashboards search "kubernetes node cpu"
```

### Find widgets whose queries cover a topic

```bash
cx dashboards query-search --description "http 5xx error rate"
cx dashboards query-search --description "p99 latency over time"
```

### Find all widgets that reference a field

```bash
cx dashboards query-search --field '$d.http.status_code'
cx dashboards query-search --field '$d.kubernetes.pod.name'
```

This reveals query patterns already in use — useful for reusing existing approaches when adding new widgets.

### Inspect and clone a found dashboard

```bash
# Get full JSON of a dashboard you found
cx dashboards get <dashboard-id> -o json > dashboard.json
# Modify it, then create a copy
cx dashboards create --from-file dashboard.json
```

## Key Principles

- **Search before creating** — always run `cx dashboards search` before building a new dashboard to avoid duplicates
- **Use field search for discovery** — `cx dashboards query-search --field` shows how a field is already being queried, which is the fastest way to find reusable PromQL or DataPrime patterns
- **Description search is fuzzy** — the results are ranked by similarity, not exact match; try several phrasings if the first search returns nothing useful
- **Use `cx dashboards get` to inspect** — once you find a relevant dashboard or widget, pull its full JSON to study the query structure

## Related Skills

- **`cx-dashboards`** — build and deploy a new Coralogix dashboard from scratch
- **`cx-telemetry-querying`** — discover what telemetry fields and metrics exist before searching dashboards
