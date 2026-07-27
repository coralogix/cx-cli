---
name: cx-infra
description: >
  Query Coralogix infrastructure resources with the `cx infra` CLI — discover
  monitored resource types, list resources, check per-resource data. Use when the user asks
  to "show resource types", "list infrastructure resources", "what resources of this kind are monitored",
  "list resources of this kind", "is this resource healthy",
  "resource health history", "when did this resource go critical",
  "get raw resource data", "infrastructure inventory", "find resources by name",
  "filter resources by service or environment", or wants to explore
  infrastructure resources and their data.
metadata:
  version: "0.1.0"
---

# Infrastructure Resources Skill

Use this skill to discover and inspect **infrastructure resources** — what exists, whether it
is healthy, and what its raw data contains.

## CLI Commands

| Command | Purpose | Key flags |
|---|---|---|
| `cx infra resources types` | List available resource types (category/type pairs) | - |
| `cx infra resources list` | List resources of one category/type | `--category`, `--type` (required); `--name-filter`, `--scope key=value`, `--start-row`, `--end-row` |
| `cx infra resources health-history <resource-id>` | Daily health samples for one resource, oldest first | - |
| `cx infra resources raw-data <resource-id>` | Raw resource document as JSON | - |

- All commands are **read-only** and support `-o json` / `-o agents` for
  structured output and `-p <profile>` (repeatable) for multi-profile fan-out.
- `--scope` is repeatable; allowed keys are `service`, `environment`, `team`
  (e.g. `--scope environment=prod --scope service=checkout`).
- Pagination: `--start-row` / `--end-row` define a row window; the default is
  the first 100 rows. Page through large fleets in windows (0-100, 100-200, …).
- Pass resource IDs **exactly as returned by `list`** (quote them — they contain
  `:` and `=`); the CLI percent-encodes them for you.

## Workflow

1. **Discover what exists** — categories and types are dynamic, so never guess:

   ```bash
   cx infra resources types -o json
   ```

2. **List resources** of the category/type you care about, narrowing with
   name and scope filters:

   ```bash
   cx infra resources list --category Hosts --type EC2_Instances \
     --name-filter web --scope environment=prod -o json
   ```

   ```bash
   # Just the ids and names
   cx infra resources list --category Hosts --type EC2_Instances -o json \
     | jq '[.[] | {resource_id, name}]'
   ```

3. **Check health** for a specific resource. Statuses are `Healthy`,
   `Critical`, or `Unmonitored`, one sample per day, oldest first:

   ```bash
   cx infra resources health-history "1001234:host_id=i-abc123" -o json

   # Only the days it was Critical
   cx infra resources health-history "1001234:host_id=i-abc123" -o json \
     | jq '[.[] | select(.status == "Critical")]'
   ```

4. **Inspect the raw document** when you need source-specific detail
   (tags, instance metadata, configuration):

   ```bash
   cx infra resources raw-data "1001234:host_id=i-abc123" -o json
   ```

5. **Pivot to other domains by name, never by resource id** — `resource_id` is
   infra-specific and no other command understands it. The interoperable keys
   are the resource **`name`** and the **`service`** scope value:

   ```bash
   # Discover which log/span fields contain the resource name
   cx search-fields "web-server-1" -s value --dataset logs

   # Query logs for the service the resource belongs to
   cx logs "filter \$l.subsystemname == 'checkout'"

   # Alert definitions mentioning the resource or its service
   cx alerts list --name "web-server-1"

   # Dashboards covering the resource or its service
   cx dashboards search "web-server-1 host health"
   ```

## Key Principles

- **Discover before listing** — `--category` and `--type` are required;
  always start from `cx infra resources types`.
- **Quote resource IDs and pass them verbatim** — they embed `:`, `|`, and `=`;
  the CLI handles URL encoding.
- **Scope keys are a fixed set** (`service`, `environment`, `team`) — unknown
  keys are rejected client-side before any request is made.
- **A missing raw document is not an error** — `raw-data` reports "no raw data"
  on stderr and emits nothing; treat it as a cleanly absent document.
- **Use `-o json` with `jq`** for filtering; use `-o agents` for token-efficient
  output in agent contexts.
- **Multi-profile fan-out** with `-p <profile>` (repeatable) tags each row with
  its profile so fleets can be compared across accounts.
- **Infra health is its own concept** — the `Healthy`/`Critical`/`Unmonitored`
  statuses are computed by the infrastructure domain and are not the same as
  Service Catalog health. Correlate them with telemetry signals;
  do not treat them as interchangeable.
- **`resource_id` never leaves this skill** — pass it only to `health-history`
  and `raw-data`. For every other command, pivot on the resource `name` or the
  `service` scope value.

## Related Skills

Bridge to these skills using the resource **name** or the **service** scope
value — never the resource id, which only this skill understands:

- **`cx-telemetry-querying`** — `cx search-fields "<name>" -s value` discovers
  which log/span fields contain the resource name; `cx logs "filter
  $l.subsystemname == '<service>'"` queries the service's telemetry. Correlate a
  `Critical` health day with error logs or CPU metrics.
- **`cx-alerts`** — `cx alerts list --name "<name-or-service>"` finds alert
  definitions matching the resource or its service by substring.
- **`cx-dashboards`** — `cx dashboards search "<name-or-service> ..."` and
  `cx dashboards query-search --description "..."` find dashboards semantically;
  pair with `search-fields -s value` to then `query-search --field` the exact
  field holding the resource name.
