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
  "what's wrong with my APM setup", "which services have no SLO",
  "did the last deployment make a service slower", "APM insights",
  "archive / dismiss an insight type", "hide an insight type", "unarchive an insight",
  or wants to explore APM entities, their metrics, and their insights.
metadata:
  version: "0.2.0"
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
| `cx service-catalog insights <entity-type>...` | Insights over every entity of the given types, plus company-wide limits | `--filter`, `--type`, `--category`, `--exclude-type`, `--exclude-category`, `--archived`, `--breakdown`, `--top-values`, `--breakdown-sort` |
| `cx service-catalog entity-insights <entity-type> <entity-id>` | Insights of one entity, with the environments each fired in | `--system` (databases), `--environment`, `--type`, `--category`, `--exclude-type`, `--exclude-category`, `--archived`, `--top-values`, `--breakdown-sort` |
| `cx service-catalog insight-drilldown <insight-type> <entity-type>` | The entities behind one insight | `--scope`, `--filter`, `--environment-match`, `--sort`, `--sort-order`, `--page-size` (100 when omitted), `--page-index`, `--name-search` |
| `cx service-catalog entity-insight-drilldown <insight-type> <entity-type> <entity-id>` | One entity's environments (or another `--dimension`) under one insight | `--scope`, `--dimension`, `--environment`, `--system` |
| `cx service-catalog insight-types archive\|unarchive` | Archive or unarchive insight types for the whole company (write) | `--type` (required, repeatable), `--yes` |

- All commands except `insight-types archive|unarchive` are **read-only**, and
  all support `-o json` / `-o toon` for structured output.
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

## Insights

Insights are findings the catalog computes for you, so start here for "what's
wrong" questions instead of pulling raw `data`:

- **Setup** categories: `instrumentation` (no language, missing compact
  metrics, no transactions), `enablement` (no spans, no CPU or memory
  profiling, missing runtime metrics, no recent ops events, no logs),
  `configuration` (no owner, no SLO, latency above the top histogram bucket),
  and `limits`: the catalog entity and identity caps are company insights
  (`companyInsights`), the cardinality caps are per-service insights under
  `perEntityType`.
- **`deployment`**: a service before and after its last deployment or version
  change - latency regression, error-rate jump, slower dependencies, and per
  operation or call regressions. It is slower than the setup categories;
  request it in a call of its own (`--category deployment`) and leave it out of
  the others (`--exclude-category deployment`).
- **Deployment insights do not check every service.** On the services list
  (`insights`), only the most recently deployed services are compared:
  currently up to 50 by default (within a 500 service x environment budget),
  whose latest deployment is within the last 7 days and at least 1 hour old by
  default; environments with very low traffic (under about 0.2 requests/s) are
  skipped. `behaviorComparisonCoverage` (`compared` of `eligibleChanges`, the
  "N of M eligible changes compared" line in text) shows how much was checked.
  Never conclude a service did not regress from its absence in the list: it may
  not have been compared. To check one service, use `entity-insights service
  <entity-id> --category deployment`.
- **Deployment insights are behind a per-account feature flag** (rolling out,
  soon generally available). When it is off they are simply absent: no
  deployment insights, no `behaviorComparisonCoverage` (no coverage line), no
  unchecked entry. Don't mention this unprompted; only when asked about
  deployment changes or regressions and none were computed, say they are not
  enabled for the account - never that nothing regressed.
- **Opt-in types**: `traffic-drop`, `mix-shift` and `dependency-regression` are
  returned only when named with `--type`, never by `--category deployment` or
  the default request.
- An insight missing from a response did not fire only if its type was
  computed: requested (opt-in types by `--type`) and served to the company.
  `no-logs` is served only to companies with logs-by-environment enabled, so its
  absence elsewhere does not mean every service sends logs.
- **Archived insight types are hidden by default.** A company can archive
  (dismiss) insight types. `insights` and `entity-insights` leave them out
  unless asked, and list the company's whole archived set in `archivedTypes`
  ("Archived (hidden by default): ..." in text). A hidden type did not
  "not fire": it was not shown. When asked about archived or dismissed
  insights, or about a type listed as archived, pass `--archived include`
  (archived types with the rest) or `--archived only` (just them).
  `archiveUnavailable: true` means the archive could not be read: every type was
  served and `archivedTypes` is empty (`--archived only` fails then).
- **Archiving is a write, per company and per insight type** (not per entity):
  `insight-types archive --type <type>` hides that type for everyone in the
  company, `insight-types unarchive --type <type>` shows it again. Both are
  idempotent, need the `service-catalog:Update` permission and `--yes` (ask
  the user first; `--read-only` blocks them), and print the company's whole
  archived set after the change (an empty set comes back as `{}` in json/toon).
  To see what is archived, read `archivedTypes` from any `insights` call.

Insight types and categories take the short form (`no-slo`,
`latency-regression`, `deployment`) or the full proto name. Text output shows
each insight's **scope** (`request-kind=server`, `language=java`,
`operation=GET /cart`, `database=postgresql/orders`, `external-services`) in
the exact syntax `--scope` takes, so a drilldown is a copy-paste. The one
exception: `language=` scopes (`missing-runtime-metrics`) are for
`insight-drilldown` only; omit `--scope` for `entity-insight-drilldown`.

```bash
# 1. What is wrong across services? (one environment: --filter deployment_environment_name=<env>)
cx service-catalog insights service --exclude-category deployment
cx service-catalog insights service --category deployment

# 2. Which services are behind one insight? Pass --scope when the insight shows one.
cx service-catalog insight-drilldown latency-regression service --scope request-kind=server

# 3. Everything about one service, including which operations and dependencies regressed
cx service-catalog entity-insights service <entity-id> --category deployment

# 4. Archived (dismissed) types, hidden by the calls above
cx service-catalog insights service --archived only

# 5. Which environments of that service an insight fired in
cx service-catalog entity-insight-drilldown operation-regression service <entity-id> \
  --scope 'operation=GET /cart'
```

- `-o json` returns the API response as-is: camelCase fields and full proto
  enum names, with the before/after measurements of deployment insights
  (`overall`, `environmentMeasurements`, `serviceMeasurements`) and, for
  `entity-insights`, the `behaviorSummary` of the compared windows. Numeric
  fields equal to 0 are omitted (e.g. a measurement's `before: 0`): read an
  absent number as 0.
- Text output of `insights` ends with how many service changes the deployment
  insights compared (`275 of 2000 eligible changes compared`), then, with
  `--breakdown`, each insight's top values per dimension
  (`owner (19 values, 26 unattributed): team-a 6/9 (66.7%)`: affected of all
  entities with that value; unattributed = affected entities with no value). An
  `insight-drilldown` page footer shows its rows within the total
  (`101-200 of 977 entities`); a page past the end names the last
  `--page-index`.
- `--filter` on `insights` and `insight-drilldown` accepts a fixed label set,
  not the `schema` labels: `deployment_environment_name`,
  `deployment_environment_type`, `k8s_cluster_name`, `service_name`,
  `service.name`, `service_namespace`, `tenant_name`, and for `database` also
  `db_system`, `db.system`, `db_name`, `db_namespace`. Any other label is a 400
  that lists the accepted ones. Filter environments with
  `--filter deployment_environment_name=<env>`, never `environment=`.
- **Unchecked insights** (`uncheckedInsights`, "Not checked" in text) mean a
  source could not be read, not that the entity is fine.
- Databases are identified by name and system: pass `--system`.

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
- **Use `-o json` with `jq`** for filtering; use `-o toon` for token-efficient
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
