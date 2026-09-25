---
name: cx-infra
description: >
  Query Coralogix infrastructure resources with the `cx infra` CLI: resource
  types, filterable attributes, resource lists, health, raw data and
  configuration changes. Use when the user asks to "show resource types",
  "list infrastructure resources", "what resources of this kind are monitored",
  "is this resource healthy", "why is this resource critical",
  "which health policy is failing", "when did this resource go critical",
  "get raw resource data", "find resources by name",
  "filter resources by service or environment", "critical hosts in a region",
  "what can I filter resources by", "resources in this cluster or namespace",
  "which resources changed recently", "what changed in this resource",
  "configuration drift", "did anything change before the incident",
  "what was this config last week", or wants to explore infrastructure resources.
metadata:
  version: "0.2.0"
---

# Infrastructure Resources

All commands live under `cx infra resources` and are read-only. Run
`cx infra resources <command> --help` for flags.

| Command          | Answers                                                        |
|------------------|----------------------------------------------------------------|
| `types`          | which `(category, type)` pairs exist                           |
| `filters`        | which attributes can be filtered on, and their accepted values |
| `list`           | which resources match, with their health policies              |
| `health-history` | daily health per resource, oldest first                        |
| `raw-data`       | the raw resource document, now or at a past time               |
| `config-changes` | which resources changed configuration in a window              |
| `config-diff`    | what changed, field by field                                   |

## Resource ids

- Take them from `list` (`resource_id`) and pass them as-is, quoted.
- `health-history`, `config-changes` and `config-diff` take up to 100 ids per
  call. Pass them all in one call, not one call each. Over 100, the CLI refuses.
  Split into batches.
- An id belongs to one team, so the id commands refuse more than one `-p`.
  Only `types`, `filters` and `list` fan out across profiles. From a
  multi-profile `list`, use the row's `profile` to pick the one to query.
- Ids mean nothing outside this skill. To reach other skills, use the resource
  `name` or its `Service` value.

## Finding resources

1. `filters` first. Attribute names and values differ per type, so never guess.
   Per attribute: `kind` (`string` is free text, `status` a fixed set),
   `values` (the accepted set) and `wildcard` (whether `*` works).
2. `list` with the filters:
   - Every `--match-all` must match, at least one `--match-any` must match, and
     the two groups are ANDed.
     `--match-all OS=linux --match-any Health=Critical --match-any Region=eu-west-1`
     means `OS=linux AND (Health=Critical OR Region=eu-west-1)`.
   - Commas give one attribute several values. In `--match-any` any one
     matches. In `--match-all` all must, which only makes sense for wildcards:
     `--match-all 'Name=*alert*,*processing*'`.
   - Name an attribute once, in one flag. A repeat is refused.
   - `--category` and `--type` are plain filters, not requirements, and are not
     attributes (no `--match-all Category=...`). At least one filter or scope
     flag is required.
   - Exact values are case-sensitive (`OS=linux`, not `OS=Linux`). Wildcards
     are not. Quote wildcards so the shell leaves `*` alone.
   - `--name-filter` and `--scope` (`service`, `environment`, `team`) are legacy
     flags. They need both `--category` and `--type`, and cannot mix with
     `--match-*`. `--match-all 'Name=*web*'` does what `--name-filter web` does.
3. Zero rows is an answer. Don't retry. A wrong attribute or value is refused
   with the accepted ones listed.

`list -o json` is an envelope: `{total_count, returned_count, resources}`.
The other commands return plain arrays or objects.

- It returns one window, rows 0 to 100 by default (`--start-row`, and
  `--end-row` which is exclusive). Keep paging while
  `start_row + returned_count < total_count`.
- A window cannot pass row 10,000. For bigger results, narrow the query.
- With several profiles, each one pages against its own `total_count` (see
  `counts_by_profile`).

## Health

- `list` rows carry `health_policies: [{id, name, status}]` with `status` one of
  `healthy`, `critical` or `pending` (not evaluated yet). A critical policy says
  why a resource is critical. No extra call needed.
- `health-history` returns `[{resource_id, health_history: [{timestamp, status}]}]`
  with `status` one of `Healthy`, `Critical` or `Unmonitored`. The two sets of
  statuses have different casing.
- Ids that don't parse, and repeats, are dropped. A shorter answer is normal and
  stderr gives the count. Returned ids are normalized, so they may not
  string-match the ones sent.
- Infra health is not Service Catalog health. Correlate it with telemetry
  rather than treating the two as the same.

```bash
cx infra resources list --match-all Health=Critical -o json \
  | jq '.resources[] | {name, failing: [.health_policies[] | select(.status == "critical") | .name]}'

cx infra resources health-history "1001234:host_id=i-abc" "1001234:host_id=i-def" -o json \
  | jq '.[] | {resource_id, critical: [.health_history[] | select(.status == "Critical") | .timestamp]}'
```

## Raw data

`raw-data -o json` returns `{version_timestamp, raw_data}`.

- `--timestamp` (`now-7d` or ISO-8601) gets the newest version at or before that
  time. `version_timestamp` says which version came back, and can be passed back
  as `--timestamp` to get it again.
- `raw_data: null` means no document. That is not an error. `version_timestamp:
  null` with a document means the document has no version.

## Configuration changes

Two steps: find out which resources changed, then see what changed.

```bash
cx infra resources config-changes --resource-id "1001234:host_id=i-abc" \
  --resource-id "1001234:host_id=i-def" --from now-24h -o json

cx infra resources config-diff --resource-id "1001234:host_id=i-abc" --from now-24h -o json \
  | jq '.[] | select(.outcome == "changed") | .changes[] | {field, before, after}'
```

- `config-changes` lists only resources that changed. Empty means nothing
  changed. Don't retry with a wider window.
- Rows are per resource and source. A resource with two collectors can give two
  rows.
- Some cloud and agent types report an update every collection cycle, so
  `config-changes` can flag them when nothing changed. Check with `config-diff`
  before telling the user something changed.
- `config-diff` has no row for a resource with no history in the 14 days before
  `--from` or inside the window. Read `outcome` before `changes`:

| `outcome`               | meaning                                                              |
|-------------------------|----------------------------------------------------------------------|
| `changed`               | the versions before and inside the window differ                     |
| `unchanged`             | they don't differ, or nothing happened inside the window             |
| `created`               | the resource first appeared inside the window                        |
| `priorStateUnavailable` | it changed, but no version exists in the 14 days before. Not a failure, and a wider `--from` won't help |
| `comparisonUnavailable` | both versions exist, but one could not be read                       |

- `before` and `after` are raw JSON. `null` means the field was added or
  removed. A new or removed subtree is one entry.
- To look at a single change, narrow `--from` and `--to` around it.

## Related Skills

Bridge with the resource name or its Service value:

- **`cx-telemetry-querying`**: `cx search-fields "<name>" -s value` finds the
  log and span fields holding the name. `cx logs "filter $l.subsystemname == '<service>'"`
  gets its telemetry.
- **`cx-alerts`**: `cx alerts list --name "<name-or-service>"`.
- **`cx-dashboards`**: `cx dashboards search "<name-or-service> ..."`. After
  `search-fields -s value`, `cx dashboards query-search --field <field>` finds
  queries on that field.
