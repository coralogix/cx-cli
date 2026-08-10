---
name: cx-service-catalog
description: >
  Query Coralogix's Service Catalog (APM v2 entities) with the `cx service-catalog`
  CLI — discover entity types, list known entities, check their schema, and pull
  aggregated or timeseries data for services, databases, operations, JVMs, and
  Kubernetes pods. Use when the user asks to "list services", "what entity types
  exist", "show me service latency", "check error rate for a service",
  "which pods are using the most memory", "database operation performance",
  "JVM GC pauses", "service health over time", "compare services by latency",
  "what columns are available for this entity type", "service catalog schema",
  or wants to explore APM entities and their metrics.
metadata:
  version: "0.1.0"
---

# Service Catalog Skill

Use this skill to discover and query **Service Catalog entities** — services,
databases, operations, database operations, JVMs, JVM GC, Kubernetes pods, and
transactions — and their columnar metrics (latency, error rate, health, resource
usage, etc.) via the v2 Service Catalog API.

## CLI Commands

| Command | Purpose | Key flags |
|---|---|---|
| `cx service-catalog entity-types` | List entity types this account has data for | - |
| `cx service-catalog schema <entity-type>` | Columns/labels schema for one entity type | - |
| `cx service-catalog entities <entity-type>` | Known entities of one type (e.g. service names) | - |
| `cx service-catalog data <entity-type>` | Aggregated column data across every entity of a type | `--start`, `--end`, `--column` (required, repeatable); `--group-by`, `--filter`, `--aggregation`, `--limit`, `--sort-column`, `--sort-order` |
| `cx service-catalog entity-data <entity-type> <entity-id>` | Column data for one named entity (drilldown) | `--start`, `--end`, `--column` (required, repeatable); `--group-by`, `--filter`, `--aggregation` |

- All commands are **read-only** and support `-o json` / `-o agents` for
  structured output.
- **Entity type accepts short forms**: `service`, `database`, `operation`,
  `database-operation`, `jvm`, `jvm-gc`, `k8s-pod`, `transaction` (case-insensitive,
  hyphens or underscores). The full proto name (`ENTITY_TYPE_K8S_POD`) also works.
  Unknown values are rejected client-side before any request is made.
- `--start`/`--end` accept `now`, `now-1h`-style relative expressions, or
  RFC3339 timestamps.
- `--column` is **required** and repeatable — discover valid column ids with
  `cx service-catalog schema <entity-type>` first; the API rejects unknown ones.
- `--filter label=value1,value2` is repeatable **across distinct labels only**
  (filters AND together); combine multiple values for the same label with commas
  rather than repeating the flag — repeating a label is rejected client-side.
- `--aggregation` is `table` (default behavior when combined with `--limit`/
  `--sort-column`/`--sort-order`) or `timeseries`. **`--limit`, `--sort-column`,
  and `--sort-order` only apply to `table`** — the backend silently ignores them
  for `timeseries`, so the CLI rejects that combination up front rather than
  sending a request whose flags are quietly dropped.
- `entity-data` percent-encodes the entity id for you — pass it as returned by
  `entities` (e.g. `checkout/api`), quoted if it contains `/`.

## Inspection Workflow

Four steps, and only because each one supplies an input the next one requires:
`entity-types` gives valid `<entity-type>` values, `schema` gives valid
`--column` ids, `entities` gives the `entity-id` for a drilldown.

1. **Discover what entity types exist** — never guess, they vary by account:

   ```bash
   cx service-catalog entity-types -o json
   ```

2. **Check the schema** for one entity type to find valid column ids and
   filterable/groupable labels:

   ```bash
   cx service-catalog schema service -o json
   ```

3. **List known entities** of that type (e.g. service names):

   ```bash
   cx service-catalog entities service -o json
   ```

4. **Query data** — aggregated across all entities, or scoped to one. Column
   ids, filter/group-by labels, and entity ids below are placeholders — always
   substitute values returned by `schema`/`entities` for the entity type in
   question, they vary by account and entity type:

   ```bash
   cx service-catalog data <entity-type> --start now-1h --end now \
     --column <column-id> --column <column-id> -o json

   cx service-catalog entity-data <entity-type> <entity-id> --start now-1h --end now \
     --column <column-id> -o json
   ```

## Examples

The commands below use `service` and `k8s-pod` for concreteness, but every
`<column-id>`, `<filterable-label>`, `<groupable-label>`, and `<entity-id>`
must come from that entity type's own `schema`/`entities` output — never
assume a column or label from one entity type exists on another.

### Top 5 entities by a metric in the last hour

```bash
cx service-catalog schema service -o json  # discover column ids first
cx service-catalog data service --start now-1h --end now \
  --column <column-id> --aggregation table \
  --sort-column <column-id> --sort-order desc --limit 5 -o json
```

### Filter to one label value

```bash
cx service-catalog schema service -o json  # discover filterable_labels first
cx service-catalog data service --start now-1h --end now \
  --column <column-id> --column <column-id> \
  --filter <filterable-label>=<value> -o json
```

### Group by a label

```bash
cx service-catalog schema service -o json  # discover groupable_labels first
cx service-catalog data service --start now-1h --end now \
  --column <column-id> --group-by <groupable-label> -o json
```

### Kubernetes pod resource saturation

```bash
cx service-catalog schema k8s-pod -o json  # discover column ids first
cx service-catalog data k8s-pod --start now-1h --end now \
  --column <column-id> --column <column-id> --column <column-id> -o json
```

### Latency over time for one entity

```bash
cx service-catalog entities service -o json  # discover entity ids first
cx service-catalog entity-data service <entity-id> --start now-24h --end now \
  --column <column-id> --aggregation timeseries -o json
```

### Just the rows

```bash
# Table responses live under .rows; timeseries under .series
cx service-catalog data service --start now-1h --end now \
  --column <column-id> -o json | jq '.rows'
```

## Key Principles

- **Discover before querying** — `entity-types` and `schema` are cheap and
  answer "what's valid here" before spending a `data`/`entity-data` call on a
  guess.
- **`--column` values are per-entity-type** — a column valid for `service` may
  not exist for `k8s-pod`; always re-check `schema` when switching entity types.
- **Malformed responses are errors, not silent empty results** — a column that
  is neither a value nor an error (or both) fails loudly rather than producing
  a partial or empty row, so a non-zero exit means investigate, not "no data".
- **A column-level error is not a command failure** — an individual column can
  come back as `{"error": "..."}` inside an otherwise successful row (e.g. a
  query timeout for just that column); check per-column before assuming the
  whole request failed.
- **`table` vs `timeseries` are mutually exclusive result shapes** — `table`
  responses are flat rows suitable for `-o json | jq '.rows'`; `timeseries`
  responses nest datapoints per series and are best consumed as raw JSON rather
  than forced into a table.
- **Use `-o json` with `jq`** for filtering; use `-o agents` for token-efficient
  output in agent contexts.
- **Multi-profile fan-out** works on every subcommand — repeat `-p <profile>` to
  compare the same entity type/data across accounts; rows and series are tagged
  with `profile` when more than one is given.

## Related Skills

- **`cx-infra`** — infrastructure resource health (hosts, containers) is a
  distinct concept from Service Catalog entity health; use `cx-infra` for
  host/instance-level monitoring and this skill for application/service-level
  APM entities.
- **`cx-telemetry-querying`** — once a service or pod name surfaces from this
  skill's commands, pivot to raw telemetry: `cx logs "filter $l.subsystemname ==
  '<service>'"` or `cx search-fields "<name>" -s value` to find related log/span
  fields. Correlate a latency or error spike with the underlying logs/spans.
- **`cx-alerts`** — `cx alerts list --name "<service-name>"` finds alert
  definitions matching a service surfaced by this skill.
- **`cx-dashboards`** — `cx dashboards search "<service-name> ..."` finds
  dashboards built around a service found here.
