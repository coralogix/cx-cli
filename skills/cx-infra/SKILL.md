---
name: cx-infra
description: >
  Query Coralogix infrastructure resources with the `cx infra` CLI — discover
  monitored resource types and filterable attributes, list and filter resources,
  check per-resource data. Use when the user asks
  to "show resource types", "list infrastructure resources", "what resources of this kind are monitored",
  "list resources of this kind", "is this resource healthy",
  "resource health history", "when did this resource go critical",
  "get raw resource data", "infrastructure inventory", "find resources by name",
  "filter resources by service or environment", "unhealthy resources",
  "critical hosts in a region", "what can I filter resources by",
  "resources in this cluster or namespace", or wants to explore
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
| `cx infra resources list` | List resources, narrowed by any filterable attribute | all optional: `--match-all NAME=VALUE`, `--match-any NAME=VALUE`, `--category`, `--type`, `--start-row`, `--end-row` |
| `cx infra resources filters` | List the attributes resources can be filtered by, with their accepted values | `--category`, `--type` |
| `cx infra resources health-history <resource-id>` | Daily health samples for one resource, oldest first | - |
| `cx infra resources raw-data <resource-id>` | Raw resource document as JSON | - |

- All commands are **read-only** and support `-o json` / `-o toon` for
  structured output.
- **Multi-profile fan-out applies to `types`, `filters` and `list` only.** Repeat
  `-p <profile>` on those to compare fleets across accounts. `health-history` and
  `raw-data` take a resource id, which is scoped to one team, so they **reject**
  more than one `-p` — run them once per profile instead.
- **Filtering takes two flags.** Every `--match-all` must match; at least one
  `--match-any` must match; the two groups combine with **AND**. So
  `--match-all OS=linux --match-any Health=Critical --match-any Region=eu-west-1`
  means `OS=linux AND (Health=Critical OR Region=eu-west-1)`. A comma within one
  flag means **either value** for that attribute: `--match-all Region=eu-west-1,us-east-1`.
  An attribute may appear **at most once per flag** — use the comma form instead.
- **Nothing is required except one narrowing input.** `--category` and `--type`
  are ordinary filters, not prerequisites, so `--match-all Health=Critical` alone
  works. A request naming none of `--match-all`, `--match-any`, `--category` or
  `--type` is rejected.
- **`Category` and `Type` are not filterable attributes.** They select which
  resource types a request covers, have their own flags, and never appear in
  `filters` output — so there is no `--match-all Category=Hosts`, use
  `--category Hosts`.
- **An exact value is case-sensitive; a wildcard one is not.** `OS=linux` matches
  and `OS=Linux` returns nothing, but `Name=*checkout*` and `Name=*Checkout*` are
  the same query. `filters` lists values only for a fixed set, so for free text
  either take the spelling from a row's `columns` or use a wildcard. A `status`
  value is matched case-insensitively either way.
- **0 rows is an answer, not a failure.** A valid attribute that is simply
  unpopulated matches nothing; do not retry or reword the query. A misspelled
  attribute or an out-of-set value is rejected outright, naming what is accepted.
- **`filters` tells you three things per attribute.** `kind` is what the value
  looks like — `string` means free text, `status` means a fixed set, and `number`,
  `bool` and `date` mean themselves. `values` lists the accepted values of a fixed
  set — pass one of those, spelled as `filters` reports it. `wildcard` says whether
  a `*` is accepted in the value — only `string` attributes accept one, and it
  matches anywhere in the value: `--match-all 'Name=*checkout*'` — quote it, or the
  shell expands the `*`.
- **`--name-filter` and `--scope` are the legacy flags and stand apart.** Both
  require `--category` and `--type`, and neither can be combined with
  `--match-all` or `--match-any`. Use `--scope` to narrow by `service`,
  `environment` or `team`; use the filter flags for everything else —
  `--match-all 'Name=*web*'` is the same query as `--name-filter web`.
- Pagination: `--start-row` / `--end-row` define a row window (`--end-row` is
  **exclusive**); the default is the first 100 rows, and omitting only `--end-row`
  gives 100 rows from `--start-row`. Page through large fleets in windows
  (0-100, 100-200, …). **`list` never pages for you** — fleets can run to hundreds
  of thousands of resources, so it returns one window and reports the total.
- **The window cannot reach past row 10,000.** The API rejects any request whose
  `start-row + rows` exceeds 10,000, so paging cannot enumerate a fleet larger
  than that even though `total_count` reports its true size. In any case,
  narrow until the result fits — add `--match-all` attributes, or a `--type` — and
  page within each subset rather than trying to walk the whole list.
- `list` wraps its rows in an envelope (`total_count`, `returned_count`,
  `resources`) — the other subcommands return bare arrays. `total_count` is the
  fleet-wide match count, independent of the window. Check it before reasoning
  over the rows: keep paging while `start_row + returned_count < total_count`, and
  if it exceeds what you can page to, narrow the query rather than trusting a
  partial answer.
- Pass resource IDs **exactly as returned by `list`** (quote them — they contain
  `:` and `=`); the CLI percent-encodes them for you.

## Inspection Workflow

Three steps, and only because each one supplies an input the next one requires:
`filters` gives the attribute names and their accepted values, `list` gives the
`resource_id`. Answering "is `web-server-1` healthy?" is these three calls —
nothing more.

1. **Discover what can be filtered** — attribute names and their accepted values
   are per resource type and dynamic, so never guess. Omit both flags for the
   union across every type:

   ```bash
   cx infra resources filters --category Hosts --type EC2_Instances -o json
   ```

   `cx infra resources types -o json` lists the `(category, type)` pairs, when
   you need those rather than the attributes.

2. **List resources**, narrowing by any attribute `filters` offered:

   ```bash
   cx infra resources list --category Hosts --type EC2_Instances \
     --match-all Health=Critical -o json
   ```

3. **Inspect one resource** using a `resource_id` from step 2. Statuses are
   `Healthy`, `Critical`, or `Unmonitored`, one sample per day, oldest first:

   ```bash
   cx infra resources health-history "1001234:host_id=i-abc123" -o json
   ```

   `raw-data` is the **alternative** to this step, not a follow-on — use it
   instead when you need source-specific detail rather than health.

## Examples

### Unhealthy hosts in one region

```bash
# Health is a fixed set, so `filters` first for the accepted values
cx infra resources filters --category Hosts -o json | jq '.[] | select(.name == "Health")'

cx infra resources list --category Hosts \
  --match-all Health=Critical --match-all Region=eu-west-1 -o json \
  | jq '.resources[] | {name, type}'
```

### Every resource whose name contains a substring

```bash
# `filters` reports wildcard: true for Name, so `*` is accepted
cx infra resources list --match-all 'Name=*checkout*' -o json \
  | jq '.resources[] | {name, category, type}'
```

### Either of two attributes, any resource type

```bash
# --match-any ORs across attributes; no category or type needed
cx infra resources list \
  --match-any Name=coredns --match-any Namespace=kube-system -o json \
  | jq '.resources[] | {name, category, type}'

# Two values of the *same* attribute take the comma form, not a second flag
cx infra resources list --match-all Namespace=kube-system,observability -o json
```

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

- **Discover before filtering** — never guess an attribute name or a status
  value; start from `cx infra resources filters`, which lists both.
- **Quote resource IDs and pass them verbatim** — they embed `:`, `|`, and `=`;
  the CLI handles URL encoding.
- **A missing raw document is not an error** — `raw-data` exits 0 and emits an
  *empty result* on **stdout**: `[]` in `json`, `[0]:` in `agents`, and
  `No raw data found.` in text. Only the note `no raw data for this resource` goes
  to stderr. Parse the empty stdout result as a cleanly absent document, not a
  failure — and do not expect stdout to be blank.
- **Use `-o json` with `jq`** for filtering; use `-o toon` for token-efficient
  output in agent contexts.
- **The row window applies per profile** — a multi-profile `list` adds a
  `counts_by_profile` breakdown, so page each profile against its own
  `total_count`, not the aggregate.
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
  `Service` attribute value.

## Related Skills

Bridge to these skills using the resource **name** or the **Service** attribute
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
