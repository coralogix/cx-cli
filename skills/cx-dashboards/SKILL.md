---
name: cx-dashboards
description: >
  Build and deploy a Coralogix dashboard for a given service from its logs,
  spans, metrics, and service specs. Discovers telemetry via cx CLI commands,
  emits importable Coralogix JSON, verifies every PromQL and DataPrime query live
  through the `cx` CLI, and creates or updates dashboards via `cx dashboards create`
  and `cx dashboards replace`. Use whenever the user asks to create, build, generate,
  deploy, update, replace, or modify a Coralogix dashboard, monitoring dashboard,
  or observability dashboard for a service, app, or pipeline.
metadata:
  version: "0.2.3"
---

# Create Coralogix Dashboard

Produces a Coralogix dashboard for a target service and deploys it via the `cx` CLI. Workflow: discover the service's telemetry, align on intent with the user, draft a plan, emit the JSON, live-verify every query through `cx`, then create the dashboard in a chosen folder.

Only use metric names, log fields, and span attributes you can cite from the service's code, README, configuration, or a live query that returned a result. Do not invent them.

---

## Reference files

Load these files for domain-specific guidance:

| Task | Reference |
|---|---|
| DataPrime query syntax | [`references/dataprime-reference.md`](references/dataprime-reference.md) |
| PromQL query syntax, counters vs gauges, histograms | [`references/promql-guidelines.md`](references/promql-guidelines.md) |
| Log field discovery, query patterns, wildfind policy | [`references/logs-querying.md`](references/logs-querying.md) |
| Span field discovery, latency analysis, trace queries | [`references/spans-querying.md`](references/spans-querying.md) |
| Dashboard-specific query gotchas (`${__range}`, `promqlQueryType`) | [`references/query-syntax.md`](references/query-syntax.md) |
| Widget JSON templates | [`references/widget-templates.md`](references/widget-templates.md) |

For choosing the right signal (metrics / logs / traces), use `cx-telemetry-querying`.

---

## Dashboard Management

Beyond creating dashboards, use these commands to manage existing ones:

| Command | Purpose |
|---|---|
| `cx dashboards catalog -o json` | List all dashboards in the catalog |
| `cx dashboards get <id> -o json` | Get a dashboard definition (useful as a template) |
| `cx dashboards folders list -o json` | List dashboard folders |
| `cx dashboards folders create --name "Name"` | Create a dashboard folder |
| `cx dashboards folders create --name "Sub" --parent-id <id>` | Create a nested folder |
| `cx dashboards replace --from-file dashboard.json` | Replace an existing dashboard with updated JSON |
| `cx dashboards check --from-file dashboard.json` | Validate a dashboard definition without persisting (server-side strict check; exits non-zero on errors) |
| `cx dashboards check <dashboard-id>` | Validate a stored dashboard by id |

To update an existing dashboard:

```bash
cx dashboards get <dashboard-id> -o json > dashboard.json
cp dashboard.json dashboard.orig.json
# Edit dashboard.json (change name, modify widgets, etc.)
cx dashboards check --from-file dashboard.json
diff <(jq -S . dashboard.orig.json) <(jq -S . dashboard.json)
cx dashboards replace --from-file dashboard.json
```

`replace` overwrites the whole dashboard with no conflict check, so run `get` right before `replace` and change only what the user asked for in the fetched JSON. Edits made since an older copy was fetched are silently lost. If `check` reports an issue in a part the diff does not show as changed, the fetched JSON is damaged: stop and tell the user rather than deleting that part to pass the check. Full steps: [`references/deploy.md`](references/deploy.md) section 5.

To duplicate a dashboard as a new copy:

```bash
cx dashboards get <dashboard-id> -o json > dashboard.json
# Remove the "id" field, then create as new:
cx dashboards create --from-file dashboard.json
```

---

## Workflow

Track progress through this checklist:

```
Dashboard Progress:
- [ ] Phase 1: Discover telemetry & business meaning
- [ ] Phase 2: Gather dashboard specifications from user
- [ ] Phase 3: Draft internal dashboard plan (sections/rows/widgets)
- [ ] Phase 4: Generate the Coralogix JSON
- [ ] Phase 5: Live-verify every query through the cx CLI
- [ ] Phase 6: Self-verify structure against the checklist
- [ ] Phase 7: Server-side validation via `cx dashboards check`
- [ ] Phase 8: Deploy via `cx dashboards create`
- [ ] Phase 9: Share the dashboard link with the user
```

Proceed in order. Run Phase 8 only after Phases 5–7 all pass. Phase 3 says when to stop for approval. Phase 9 is mandatory — the workflow is not done until the user has a clickable link.

---

## Phase 1: Discover telemetry & business meaning

For the target service, gather:

1. **Business purpose** - read `README.md` and the top-level entrypoint (`main.*`, `index.*`, `cmd/main.go`, etc.). Summarize in 2–3 sentences what it does, its key stages, and what can go wrong.
2. **Metrics** - for each candidate keyword (service name, subsystem, verbs like `request`, `error`, `latency`, `dlq`) run `cx metrics search --name '*<keyword>*'`. When a metric looks promising, list its labels with `cx metrics get-labels <metric>`. Only use names `cx metrics search` returns - this is what prevents invented metrics from reaching Phase 5. Cross-check the service's instrumentation (`prometheus_client`, `promauto.NewCounter/Histogram/Gauge`, OTel meters, `prom-client`, Micrometer, `metrics.py`) for semantics and histogram buckets (`_sum`, `_count`, `_bucket`).
3. **Logs** - discover custom `$d.*` fields with `cx search-fields "<description>" --dataset logs` before assuming a field exists. Sample message templates and severity with `cx logs "filter \$l.applicationname == '<app>'" --limit 5 -o json`. Standard fields (`$m.severity`, `$m.timestamp`, `$l.applicationname`, `$l.subsystemname`) don't need discovery.
4. **Spans / traces** - discover span attributes with `cx search-fields "<description>" --dataset spans`. Sample with `cx spans "filter \$l.serviceName == '<svc>'" --limit 5 -o json`. Error conventions vary (`$d.tags.error`, `$d.http.status_code`); check samples before filtering.
5. **Message buses & DLQs** - grep for Kafka, RabbitMQ, SQS, Pub/Sub clients and any `dlq`/`DLQ` references. Note topic/queue names for DLQ panels.
6. **Service configuration** - check `meta.yaml`, Helm `values.yaml`, `Deployment`, `Dockerfile`, `chart.yaml`. Extract:
   - The `applicationname` / `subsystemname` label values as they appear in Coralogix.
   - Tenant/account/team identifiers used as metric or log labels.
   - Deployment environments (`prod`, `staging`, `dev`, …).

If the signal for a question is ambiguous (e.g. "how much revenue last week"), delegate to `cx-telemetry-querying` first.

Produce a short internal summary before moving on. If critical telemetry is missing (e.g. no metrics), surface that to the user and ask whether they want a log-only or trace-only dashboard.

---

## Phase 2: Gather dashboard specifications

Ask the user a focused set (≤6). Prefer `AskQuestion`:

1. **Audience & use** - on-call triage, product/business tracking, capacity planning, customer success?
2. **Default time range** - typical viewing window (e.g. 24h, 7d). Queries still use `${__range}` so users can zoom.
3. **Slicing dimensions** - top-level filters (`tenant_id`, `account_id`, `subsystem_name`, `region`, `env`, …).
4. **Environment scope** - which environments to include/exclude (common default: exclude `dev`, `staging`, `test`).
5. **SLO-ish signals** - success-rate, latency, or throughput targets to highlight?
6. **Priorities** - what to see first (drives row ordering and which section is `collapsed: true`).

Don't block on answers you can reasonably infer - state the inference and continue. When the request already names what the dashboard shows ("a dashboard for X with traffic, latency and errors"), skip the questions and list your assumptions in the plan.

---

## Phase 3: Draft the internal plan

Write a markdown plan before JSON generation:

```
## Dashboard: <Service> - <Purpose>

### Section 1: <Overview> (collapsed: false)
- Row 1: [widget type] <title> - <what it shows> - source: metrics|logs|spans
- Row 2: ...

### Section 2: <Deep dive> (collapsed: false)
...

### Section N: <Logs & errors> (collapsed: true)
...

### Top-level filters
- <label> (<source>)

### Assumptions / gaps
- ...
```

**Section design**:
- First section: at-a-glance health (gauges + key rates), always expanded.
- Pair related time-series in the same row (rate + latency).
- Final section (raw logs, rare breakdowns): `collapsed: true`.
- Aim for 3–5 sections, 6–20 widgets total.
- Headline numbers share one row of up to 4 stat widgets at row height `12`; a chart row holds 1–2 widgets.

**Widget-type selection**:

| Signal | Widget type |
|---|---|
| Single headline number from a metrics query (count, % success, totals) | `gauge` (Coralogix calls this "stat") |
| Breakdown across ≤8 categories from a metrics query | `pieChart` |
| Change over time from a metrics query (rate, latency, count per bucket) | `lineChart` |
| Top-N tables and listings, from any query (PromQL `topk` included) | `dynamic` with a `table` visualization (never legacy `dataTable`) |
| Query over logs, spans or DataPrime | `dynamic`, visualization per the shape table |

Don't use other widget types unless the user asks.

**Logs, spans and DataPrime widgets are `dynamic`, and visualized.** Never emit the legacy `dataTable` widget, for any query: a PromQL top-N table is a `dynamic` widget with a `metrics` query and a `table` visualization. Use `definition.dynamic` and set its `visualization` to the chart the query's shape calls for; `table` is right only for raw rows the user wants to read. Alias every column in the query and map the visualization to the aliases. Shape-to-visualization table and templates: `references/widget-templates.md` → *Widget: dynamic*.

**When to stop for approval.** A request to create, build or deploy the dashboard is the approval: show the plan with its assumptions and continue to Phase 4 in the same turn, because an unattended run that stops here delivers no dashboard. Stop and wait in two cases: the user asked to review the plan first, or a choice you cannot infer changes what gets built (two services with the same name, no telemetry for a signal they named).

---

## Phase 4: Generate the Coralogix JSON

Produce a single JSON document following [`references/widget-templates.md`](references/widget-templates.md). Key rules:

1. **Top-level shape**:
   ```
   {
     "id": "<21-char-nanoid>",
     "name": "<Dashboard Name>",
     "layout": { "sections": [ ... ] },
     "variables": [],
     "variablesV2": [],
     "filters": [ ... ],
     "relativeTimeFrame": "<seconds>s",
     "annotations": [],
     "off": {},
     "actions": []
   }
   ```
2. **IDs** - fresh UUIDs for every `section`, `row`, `widget`, and query `id`.
3. **Row height** - `"appearance": { "height": 19 }` unless there's a reason to change.
4. **Section options** - include `options.custom.name`, `collapsed`, and `color.predefined: "SECTION_PREDEFINED_COLOR_UNSPECIFIED"`.
5. **Filters** - one entry per slicing dimension the user asked for in Phase 2, and only when it applies to most widgets: a `metrics` filter narrows PromQL widgets only. Default operator `equals` with empty `values` so users can fill in. Use `notEquals` for environment exclusions (see [`references/widget-templates.md`](references/widget-templates.md)). With no requested dimension, emit `"filters": []`.
6. **relativeTimeFrame** - default `"172800s"` (48h) unless the user specified otherwise.
7. **Units** - set `unit` on every widget whose value has one (latency, bytes, percent, currency), and keep the unit out of the title: `"Checkout p95"` with `UNIT_MILLISECONDS`. Enum: `references/widget-templates.md` → *Unit enum quick-reference*.
8. **Series names** - a grouped PromQL chart sets `seriesNameTemplate` to the grouping label (`"{{service_name}}"`); each query of a multi-query chart gets a `name` and a `seriesNameTemplate` that say which query it is (`"p95"`). Otherwise the legend shows the raw label set or `Query 1`, `Query 2`.
9. **Descriptions** - a widget gets a one-sentence `description` when its title leaves open what is counted, what is excluded, or which statuses count as errors.
10. **Thresholds on headline numbers** - a stat or gauge for an error count, error rate, success rate or latency carries `thresholds`, so its colour says good or bad.

For query syntax follow [`references/query-syntax.md`](references/query-syntax.md); for the full query languages load [`references/dataprime-reference.md`](references/dataprime-reference.md) and [`references/promql-guidelines.md`](references/promql-guidelines.md).

---

## Phase 5: Live-verify every query through the cx CLI

Every PromQL and DataPrime query in the draft has to successfully run through `cx` before Phase 8. This catches invented metric names, typoed field paths, and malformed pipelines.

### Frequent vs Archive (what / when / where in JSON)

**What**:
- **Archive** (`TIER_ARCHIVE`, "Monitoring" in the UI): holds all ingested logs and spans.
- **Frequent** (`TIER_FREQUENT_SEARCH`): holds only data that TCO routes to the high-priority tier.

**Check the archive first.** Run this once, before you set any widget's tier:

```bash
cx archive logs get -o json
```

If `target.archiveSpec.isActive` is `false`, or the target has no bucket, the account has no archive. Archive queries then return empty with exit 0 and no warning, so verification passes and the widgets render empty. Use **Frequent** for every logs, spans and DataPrime widget, verify with `--tier frequent`, and tell the user why. `get` is a read, so it needs no `--yes`.

**When to choose** (account with an active archive):
- Use **Archive** by default. Set `dataModeType` to `DATA_MODE_TYPE_ARCHIVE` on every logs, spans and DataPrime widget, and verify with `--tier archive` (the `cx logs` / `cx spans` default).
- Use **Frequent** only when the user asks for it. A Frequent widget renders empty for data that is not in the high-priority tier.
- Metrics widgets are not tiered.

The two languages are verified against different windows:

- **PromQL**: map `relativeTimeFrame` to a `$RANGE` token (e.g. `48h` for `172800s`), substitute `${__range}` with `[$RANGE]` (and `${__rate_interval}` / `${__interval}` with `[5m]`) for the CLI call, then restore the variables in the JSON before Phase 6. Range vectors are window-sensitive, so the check has to match what the dashboard will evaluate.
- **DataPrime**: verify against a fixed short window (`now-15m` → `now`, `--limit 1`). The goal is syntax / field / pipeline validation, not data-presence on the dashboard's window — a short window is faster and a cleaner fail signal.

**Verify all queries in one command.** Put the queries in a file or a loop and run them in a single tool call, printing each widget's title before its result. Re-run only the queries that failed. One call per query costs a model round trip each.

Full procedure (CLI invocations, `$RANGE` mapping table, retry budget, failure modes): [`references/verification.md`](references/verification.md).

If a query can't be made to pass within the retry budget, surface it to the user with the CLI error verbatim - don't ship a broken widget.

---

## Phase 6: Self-verify structure

Run this checklist against the final JSON. Fix and re-check if any item fails before Phase 7.

### Query syntax (dashboard-specific)
- [ ] PromQL range vectors use `[${__range}]` in single-value widgets (gauge, pieChart, dynamic table or stat) and `[${__rate_interval}]` / `[${__interval}]` in `lineChart` widgets, where `[${__range}]` flattens the line into an average over the whole dashboard range. Never `[$__range]` (missing braces), never `[5m]` (unless the panel is intentionally a sliding window). Table: [`references/query-syntax.md`](references/query-syntax.md) §1.
- [ ] `promqlQueryType` is `PROM_QL_QUERY_TYPE_INSTANT` for single-value widgets (gauge, pieChart, dynamic table). Omitted for `lineChart`.
- [ ] DataPrime log queries use `$d.<field>` / `$l.applicationname` / unquoted severity enums (full rules: [`references/dataprime-reference.md`](references/dataprime-reference.md)).
- [ ] Every DataPrime widget query starts with `source logs` or `source spans` (dashboard widgets require the source prefix; Phase 5 verification strips it before handing the pipeline to `cx logs` / `cx spans`).
- [ ] Success-rate denominators in single-value widgets wrapped in `clamp_min(..., 1)`; unclamped in a `lineChart`.
- [ ] Histogram queries use the correct suffix (`_sum`, `_count`, `_bucket`).
- [ ] Widget queries are valid **without** the dashboard-level `filters` - Coralogix injects them at render time.

### Structure
- [ ] Each section has `id.value`, `rows`, and `options.custom`.
- [ ] Each row has `id.value`, `appearance.height`, and `widgets`.
- [ ] Each widget has a unique `id.value` and a `definition` with exactly one of `gauge` / `pieChart` / `lineChart` / `dynamic`.
- [ ] Every gauge has numeric `min` and `max`, and `min < max`.
- [ ] Success-rate gauges use `thresholdType: "THRESHOLD_TYPE_ABSOLUTE"` with green at high values; error/DLQ gauges use red at high values.
- [ ] Metrics "Total" / "stat" widgets are encoded as `gauge`. A logs, spans or DataPrime total is a `dynamic` widget with a `stat` visualization.
- [ ] Top-level `filters` holds the slicing dimensions the user asked for, and nothing else.
- [ ] Every latency, size, percent or currency widget sets `unit`, and its title carries no unit.
- [ ] Every grouped PromQL chart sets `seriesNameTemplate`; every query in a multi-query chart has a `name`.
- [ ] Every `stat` query returns one column, the one in `valueFields`.
- [ ] All IDs are freshly generated UUIDs, unique within the document.
- [ ] Every `queryDisplaySettings[].queryId` equals the `id` of a `queryDefinitions[]` entry in the same widget. `cx dashboards check` accepts a mismatch, and the widget then renders empty.

### Content
- [ ] Dashboard name is descriptive (`"<Service> - <Purpose>"`).
- [ ] Widget titles are short, human-readable, and match what the query computes.
- [ ] The logs/errors section is `collapsed: true` unless the user said otherwise.

---

## Phase 7: Server-side validation via `cx dashboards check`

Phase 6 caught structural issues by hand. This phase runs the whole dashboard through the Coralogix Dashboard Service's strict validator (`CheckDashboard`) — the same validation `create`/`replace` apply on write, plus a superset that compiles every PromQL/DataPrime query and enforces required ids for variables and filters.

1. Run `cx dashboards check --from-file /tmp/cx-dashboard-<slug>.json`.
2. If the command exits non-zero, the output lists issues with `severity`, `location` (an RFC 6901 JSON Pointer into the dashboard, e.g. `/sections/0/rows/1/widgets/2`), and `message`. Fix each issue in the JSON (loop back to Phase 4), re-run Phase 5 for any query that changed, then re-run this phase. When updating an existing dashboard, an issue in a part you did not change means the fetched JSON is damaged: stop and tell the user instead of fixing it, as in [`references/deploy.md`](references/deploy.md) section 5.
3. When `check` exits 0 (text output: `Dashboard is valid (no issues)`), proceed to Phase 8.

`check` is read-only — it never persists the dashboard. Warnings (`SEVERITY_WARNING`) print but do not fail the gate; only errors (`SEVERITY_ERROR`) cause a non-zero exit. In multi-profile fan-out, any profile returning errors fails the command.

---

## Phase 8: Deploy via `cx dashboards create`

Don't tell the user to paste JSON into the Coralogix UI - deploy it directly.

1. List folders: `cx dashboards folders list -o json`.
2. Use the folder the user named. Otherwise suggest the best match (team, product area, or a folder named after the service) and default to root (omit `--folder`) if nothing fits. A folder named after a person is never a match.
3. Write the verified JSON to a temp file and run `cx dashboards create --from-file /tmp/cx-dashboard-<slug>.json --folder <id> --yes`. Without `--yes`, `create` asks for confirmation and fails in a non-interactive shell; the user's request to create the dashboard is that confirmation. The CLI generates the `requestId` envelope and prints the created dashboard ID.

Full procedure (folder-picking UX, command templates, idempotency note): [`references/deploy.md`](references/deploy.md).

On failure: show the CLI error verbatim and return to Phase 5. The most common cause is a query that parses locally but the live API rejects.

---

## Phase 9: Share the dashboard link

The workflow is **not done** until the user has a clickable link to the dashboard. Printing the ID alone forces the user to navigate the Coralogix UI by hand, which defeats the point of automating deployment.

After Phase 8 succeeds, capture the `View in Coralogix: <url>` line that `cx dashboards create` prints to stderr (see [`references/deploy.md`](references/deploy.md) § "Share the link" for when it's omitted) and emit the output template below. Render the dashboard **name** as the link text — that's what the user clicks.

Linking to a dashboard found via `cx dashboards catalog` (rather than one you just created) works differently: `catalog` prints only one link, to the catalog page, not a per-dashboard link. To link to one specific dashboard from that list, build `<base>/dashboards/<dashboard_id>`, where `<base>` is the console URL already seen in a `View in Coralogix: <base>/...` line printed by any `cx dashboards` command this session — never fabricate `<base>` yourself, and never invent it if no such line has been printed yet.

---

## Output format for the user

When `cx dashboards create` printed a `View in Coralogix:` link:

````
## Plan
<the approved Phase 3 plan>

## Verification
- PromQL queries verified: <N>/<N>
- DataPrime queries verified: <N>/<N>

## Deployed
- Dashboard: **[<Name>](<url from the View in Coralogix line>)**
- ID: `<id>`
- Folder: `<folder name or "root">`
- Profile: `<cx profile>`

Open it: [<Name>](<url from the View in Coralogix line>)

Adjust filter values (e.g. `account_id`) after opening it.
````

When `cx dashboards create` did **not** print a `View in Coralogix:` line (no console link could be resolved for the profile and no `console_url` override configured), omit the link entirely — do not invent a URL. Use this template instead:

````
## Plan
<the approved Phase 3 plan>

## Verification
- PromQL queries verified: <N>/<N>
- DataPrime queries verified: <N>/<N>

## Deployed
- Dashboard: **<Name>** (open via the Coralogix UI; ID `<id>`)
- ID: `<id>`
- Folder: `<folder name or "root">`
- Profile: `<cx profile>`

Adjust filter values (e.g. `account_id`) after opening it.
````

---

## References

- Dashboard query gotchas & cross-references: [`references/query-syntax.md`](references/query-syntax.md)
- Widget JSON templates: [`references/widget-templates.md`](references/widget-templates.md)
- Live-verification procedure: [`references/verification.md`](references/verification.md)
- Deploy procedure: [`references/deploy.md`](references/deploy.md)
- DataPrime language reference: [`references/dataprime-reference.md`](references/dataprime-reference.md)
- PromQL reference: [`references/promql-guidelines.md`](references/promql-guidelines.md)
- Log querying patterns: [`references/logs-querying.md`](references/logs-querying.md)
- Span querying patterns: [`references/spans-querying.md`](references/spans-querying.md)
- Inline DataPrime help: `cx dataprime list`, `cx dataprime show <command>`
- Coralogix Custom Dashboards docs: <https://www.coralogix.com/docs/user-guides/custom-dashboards/introduction/>

### Related Skills

- **`cx-observability-setup`** - full monitoring setup workflow (views, webhooks, notifications, integrations)
- **`cx-slos`** - SLO-connected reliability targets to surface on dashboards
- **`cx-cases`** - triage the cases raised against the services a dashboard monitors
- **`cx-telemetry-querying`** - discover the right telemetry signal before building dashboards
