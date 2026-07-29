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
  structured output.
- **Multi-profile fan-out applies to `types` and `list` only.** Repeat
  `-p <profile>` on those to compare fleets across accounts. `health-history` and
  `raw-data` take a resource id, which is scoped to one team, so they **reject**
  more than one `-p` — run them once per profile instead.
- `--scope` is repeatable across **different** keys; allowed keys are `service`,
  `environment`, `team` (e.g. `--scope environment=prod --scope service=checkout`).
  Multiple keys combine with **AND** — a resource must match all of them. Each key
  accepts a single value and may be given **at most once**; repeating one (e.g.
  `--scope service=a --scope service=b`) is rejected. To cover several values for one key, run one query per
  value and combine the results.
- Pagination: `--start-row` / `--end-row` define a row window (`--end-row` is
  **exclusive**); the default is the first 100 rows, and omitting only `--end-row`
  gives 100 rows from `--start-row`. Page through large fleets in windows
  (0-100, 100-200, …). **`list` never pages for you** — fleets can run to hundreds
  of thousands of resources, so it returns one window and reports the total.
- **The window cannot reach past row 10,000.** The API rejects any request whose
  `start-row + rows` exceeds 10,000, so paging cannot enumerate a fleet larger
  than that even though `total_count` reports its true size. In any case,
  narrow with `--name-filter` or `--scope` and page within each subset rather
  than trying to walk the whole list.
- `list` wraps its rows in an envelope (`total_count`, `returned_count`,
  `resources`) — the other subcommands return bare arrays. `total_count` is the
  fleet-wide match count, always present and independent of the window, so use it
  as the stop condition: keep paging while `start_row + returned_count <
  total_count`, subject to the 10,000-row ceiling above.
- Pass resource IDs **exactly as returned by `list`** (quote them — they contain
  `:` and `=`); the CLI percent-encodes them for you.

## Inspection Workflow

Three steps, and only because each one supplies an input the next one requires:
`types` gives the mandatory `--category`/`--type`, `list` gives the `resource_id`.
Answering "is `web-server-1` healthy?" is these three calls — nothing more.

1. **Discover what exists** — categories and types are dynamic, so never guess:

   ```bash
   cx infra resources types -o json
   ```

2. **List resources** of that category/type, narrowing with name and scope filters:

   ```bash
   cx infra resources list --category Hosts --type EC2_Instances \
     --name-filter web --scope environment=prod -o json
   ```

3. **Inspect one resource** using a `resource_id` from step 2. Statuses are
   `Healthy`, `Critical`, or `Unmonitored`, one sample per day, oldest first:

   ```bash
   cx infra resources health-history "1001234:host_id=i-abc123" -o json
   ```

   `raw-data` is the **alternative** to this step, not a follow-on — use it
   instead when you need source-specific detail rather than health.

## Examples

### Just the ids and names

```bash
# Rows live under .resources — `list` returns an envelope
cx infra resources list --category Hosts --type EC2_Instances -o json \
  | jq '[.resources[] | {resource_id, name}]'
```

### Check fleet size, and whether one window covered it

```bash
cx infra resources list --category Hosts --type EC2_Instances -o json \
  | jq '{total_count, returned_count}'

# Next window, if there is one
cx infra resources list --category Hosts --type EC2_Instances \
  --start-row 100 --end-row 200 -o json
```

### Find when a resource went critical

```bash
# health-history returns a bare array, so no .resources here
cx infra resources health-history "1001234:host_id=i-abc123" -o json \
  | jq '[.[] | select(.status == "Critical")]'
```

### Read the raw resource document

```bash
# Source-specific detail: tags, instance metadata, configuration
cx infra resources raw-data "1001234:host_id=i-abc123" -o json
```

## Key Principles

- **Discover before listing** — `--category` and `--type` are required;
  always start from `cx infra resources types`.
- **Quote resource IDs and pass them verbatim** — they embed `:`, `|`, and `=`;
  the CLI handles URL encoding.
- **Scope keys are a fixed set** (`service`, `environment`, `team`) — unknown
  keys are rejected client-side before any request is made.
- **A missing raw document is not an error** — `raw-data` exits 0 and emits an
  *empty result* on **stdout**: `[]` in `json`, `[0]:` in `agents`, and
  `No raw data found.` in text. Only the note `no raw data for this resource` goes
  to stderr. Parse the empty stdout result as a cleanly absent document, not a
  failure — and do not expect stdout to be blank.
- **Use `-o json` with `jq`** for filtering; use `-o agents` for token-efficient
  output in agent contexts.
- **Multi-profile fan-out is for `types` and `list` only** — repeating
  `-p <profile>` tags each row with its profile so fleets can be compared across
  accounts. The row window applies per profile, so `list` adds a
  `counts_by_profile` breakdown — page each profile against its own `total_count`,
  not the aggregate. `health-history` and `raw-data` error on a second `-p`.
- **A resource id never crosses profiles** — it embeds the team id
  (`1001234:host_id=…`), so an id from one account cannot resolve in another. When
  a multi-profile `list` turns up something worth inspecting, note its `profile`
  field and query that single profile for its health or raw data.
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
