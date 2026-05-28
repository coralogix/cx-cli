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
  version: "0.2.0"
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

To update an existing dashboard:

```bash
cx dashboards get <dashboard-id> -o json > dashboard.json
# Edit dashboard.json (change name, modify widgets, etc.)
cx dashboards replace --from-file dashboard.json
```

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
- [ ] Phase 7: Deploy via `cx dashboards create`
- [ ] Phase 8: Share the dashboard link with the user
```

Proceed in order. Don't jump to Phase 4 before the user approves the Phase 3 plan, and don't run Phase 7 before Phases 5 and 6 both pass. Phase 8 is mandatory — the workflow is not done until the user has a clickable link.

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

Don't block on answers you can reasonably infer - state the inference and continue.

---

## Phase 3: Draft the internal plan

Write a markdown plan the user can approve before JSON generation:

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

**Widget-type selection**:

| Signal | Widget type |
|---|---|
| Single headline number (count, % success, totals) | `gauge` (Coralogix calls this "stat") |
| Breakdown across ≤8 categories | `pieChart` |
| Change over time (rate, latency, count per bucket) | `lineChart` |
| Top-N tables, last errors, per-entity listings | `dataTable` |

Don't use other widget types unless the user asks.

Wait for the user to approve or adjust the plan before emitting JSON.

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
5. **Filters** - one entry per slicing dimension from Phase 2. Default operator `equals` with empty `values` so users can fill in. Use `notEquals` for environment exclusions (see [`references/widget-templates.md`](references/widget-templates.md)).
6. **relativeTimeFrame** - default `"172800s"` (48h) unless the user specified otherwise.

For query syntax follow [`references/query-syntax.md`](references/query-syntax.md); for the full query languages load [`references/dataprime-reference.md`](references/dataprime-reference.md) and [`references/promql-guidelines.md`](references/promql-guidelines.md).

---

## Phase 5: Live-verify every query through the cx CLI

Every PromQL and DataPrime query in the draft has to successfully run through `cx` before Phase 7. This catches invented metric names, typoed field paths, and malformed pipelines.

### Frequent vs Archive (what / when / where in JSON)

**What**:
- **Frequent** (`TIER_FREQUENT_SEARCH`): hot tier for fast search on recent logs/spans.
- **Archive** (`TIER_ARCHIVE`): cold tier for older logs/spans (long-term).

**When to choose**:
- Choose **Frequent** for on-call and recent investigations (hours/days).
- Choose **Archive** for long lookbacks (weeks/months) or when the time range is beyond hot retention.

The two languages are verified against different windows:

- **PromQL**: map `relativeTimeFrame` to a `$RANGE` token (e.g. `48h` for `172800s`), substitute `${__range}` with `[$RANGE]` for the CLI call, then restore `${__range}` in the JSON before Phase 6. Range vectors are window-sensitive, so the check has to match what the dashboard will evaluate.
- **DataPrime**: verify against a fixed short window (`now-15m` → `now`, `--limit 1`). The goal is syntax / field / pipeline validation, not data-presence on the dashboard's window — a short window is faster and a cleaner fail signal.

Full procedure (CLI invocations, `$RANGE` mapping table, retry budget, failure modes): [`references/verification.md`](references/verification.md).

If a query can't be made to pass within the retry budget, surface it to the user with the CLI error verbatim - don't ship a broken widget.

---

## Phase 6: Self-verify structure

Run this checklist against the final JSON. Fix and re-check if any item fails before Phase 7.

### Query syntax (dashboard-specific)
- [ ] Every PromQL range vector in a metrics widget uses `[${__range}]` - never `[$__range]`, never `[5m]` (unless the panel is intentionally a sliding window).
- [ ] `promqlQueryType` is `PROM_QL_QUERY_TYPE_INSTANT` for single-value widgets (gauge, pieChart, dataTable). Omitted for `lineChart`.
- [ ] DataPrime log queries use `$d.message` / `$l.applicationname` / unquoted severity enums (full rules: [`references/dataprime-reference.md`](references/dataprime-reference.md)).
- [ ] Every DataPrime widget query starts with `source logs` or `source spans` (dashboard widgets require the source prefix; Phase 5 verification strips it before handing the pipeline to `cx logs` / `cx spans`).
- [ ] Success-rate denominators wrapped in `clamp_min(..., 1)`.
- [ ] Histogram queries use the correct suffix (`_sum`, `_count`, `_bucket`).
- [ ] Widget queries are valid **without** the dashboard-level `filters` - Coralogix injects them at render time.

### Structure
- [ ] Each section has `id.value`, `rows`, and `options.custom`.
- [ ] Each row has `id.value`, `appearance.height`, and `widgets`.
- [ ] Each widget has a unique `id.value` and a `definition` with exactly one of `gauge` / `pieChart` / `lineChart` / `dataTable`.
- [ ] Every gauge has numeric `min` and `max`, and `min < max`.
- [ ] Success-rate gauges use `thresholdType: "THRESHOLD_TYPE_ABSOLUTE"` with green at high values; error/DLQ gauges use red at high values.
- [ ] "Total" / "stat" widgets are encoded as `gauge`, not as a stat type.
- [ ] Top-level `filters` includes each slicing dimension from Phase 2.
- [ ] All IDs are freshly generated UUIDs, unique within the document.

### Content
- [ ] Dashboard name is descriptive (`"<Service> - <Purpose>"`).
- [ ] Widget titles are short, human-readable, and match what the query computes.
- [ ] The logs/errors section is `collapsed: true` unless the user said otherwise.

---

## Phase 7: Deploy via `cx dashboards create`

Don't tell the user to paste JSON into the Coralogix UI - deploy it directly.

1. List folders: `cx dashboards folders list -o json`.
2. Suggest the best folder match (team, product area, or a folder named after the service). Default to root (omit `--folder`) if nothing fits.
3. Write the verified JSON to a temp file and run `cx dashboards create --from-file /tmp/cx-dashboard-<slug>.json --folder <id>`. The CLI generates the `requestId` envelope and prints the created dashboard ID.

Full procedure (folder-picking UX, command templates, idempotency note): [`references/deploy.md`](references/deploy.md).

On failure: show the CLI error verbatim and return to Phase 5. The most common cause is a query that parses locally but the live API rejects.

---

## Phase 8: Share the dashboard link

The workflow is **not done** until the user has a clickable link to the dashboard. Printing the ID alone forces the user to navigate the Coralogix UI by hand, which defeats the point of automating deployment.

After Phase 7 succeeds, capture the dashboard `id` returned by `cx dashboards create`, build the URL using the region → webapp host mapping in [`references/deploy.md`](references/deploy.md) § "Share the link", and emit the output template below. Render the dashboard **name** as the link text — that's what the user clicks.

---

## Output format for the user

When the webapp host is resolvable (any built-in region, or a `Custom` endpoint that follows the `api.<host>` convention):

````
## Plan
<the approved Phase 3 plan>

## Verification
- PromQL queries verified: <N>/<N>
- DataPrime queries verified: <N>/<N>

## Deployed
- Dashboard: **[<Name>](https://<region>.app.coralogix.com/#/dashboards/<id>)**
- ID: `<id>`
- Folder: `<folder name or "root">`
- Profile: `<cx profile>`

Open it: [<Name>](https://<region>.app.coralogix.com/#/dashboards/<id>)

Adjust filter values (e.g. `account_id`) after opening it.
````

When the webapp host **cannot** be derived (custom endpoint that doesn't match `api.<host>`), omit the link entirely — do not invent a URL. Use this template instead:

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
- **`cx-incident-management`** - SLO and alert-connected dashboards, incident triage
- **`cx-telemetry-querying`** - discover the right telemetry signal before building dashboards
