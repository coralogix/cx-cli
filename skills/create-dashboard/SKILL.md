---
name: create-dashboard
description: >
  Build and deploy a Coralogix dashboard for a given service based on its logs,
  spans, metrics, and any available service specifications. Discovers telemetry
  with the sibling `metrics-query` / `query-logs` / `query-spans` skills,
  generates importable Coralogix JSON, verifies every PromQL and DataPrime query
  live through the `cx` CLI, and deploys the finished dashboard with
  `cx dashboards create`. Use when the user asks for a Coralogix dashboard,
  monitoring dashboard, or observability dashboard for a specific service, app,
  or pipeline.
license: Apache-2.0
metadata:
  version: "2.0.0"
  integration: coralogix
  argument_hint: "[service name or path]"
  signals:
    - logs
    - metrics
    - traces
  deployment:
    - kubernetes
    - helm
    - docker
    - ecs
    - aws
  triggers:
    description: >
      Load when the user asks to create, build, generate, or deploy a Coralogix
      dashboard (or monitoring / observability dashboard) for a specific
      service, app, or pipeline.
    always: false
    file_patterns:
      - "**/meta.yaml"
      - "**/values*.yaml"
      - "**/dashboard*.json"
    config_keys:
      - "applicationname:"
      - "subsystemname:"
      - "promqlQuery:"
      - "dataprimeQuery:"
    keywords:
      - coralogix dashboard
      - monitoring dashboard
      - observability dashboard
      - dashboard json
      - dataprime
      - promql
      - widget templates
      - create dashboard
      - deploy dashboard
  docs: https://coralogix.com/docs/user-guides/monitoring-and-insights/custom-dashboards/
---

# Create Coralogix Dashboard

Produces a **Coralogix dashboard** for a target service and deploys it into the user's Coralogix environment via the `cx` CLI. Works by first discovering the service's telemetry (metrics, logs, spans), aligning on dashboard intent with the user, drafting an internal plan, emitting the full JSON, **live-verifying every query** with `cx`, and finally creating the dashboard in a user-chosen folder.

**Hard output contract**: the final dashboard must be valid and successfully created by `cx dashboards create`. Do not invent metric names, log fields, or attributes — only use what you can cite from the service's code, README, configuration, or a live query that returns a result.

---

## Sibling skills you must delegate to

This skill is the orchestrator. Don't re-implement knowledge that already lives in dedicated skills.

| When you need to... | Use this skill |
|---|---|
| Write / understand a DataPrime query | `dataprime` (`skills/dataprime/references/dataprime-reference.md`) |
| Write / understand a PromQL query, find a metric, list labels | `metrics-query` |
| Run a DataPrime query against logs (for discovery or verification) | `query-logs` |
| Run a DataPrime query against spans (for discovery or verification) | `query-spans` |
| Decide which pillar (metrics / logs / traces) to pull from | `telemetry-querying` |
| Semantic field search (`cx search-fields`) | `query-logs` or `query-spans` |

The Coralogix-dashboard-specific pitfalls (the `${__range}` variable, `promqlQueryType`, widget filter syntax, threshold types) live in `references/query-syntax.md` and the widget JSON templates live in `references/widget-templates.md`.

---

## Workflow

Copy this checklist and track progress through it:

```
Dashboard Progress:
- [ ] Phase 1: Discover telemetry & business meaning
- [ ] Phase 2: Gather dashboard specifications from user
- [ ] Phase 3: Draft internal dashboard plan (sections/rows/widgets)
- [ ] Phase 4: Generate the Coralogix JSON
- [ ] Phase 5: Live-verify every query through the cx CLI
- [ ] Phase 6: Self-verify structure against the checklist
- [ ] Phase 7: Deploy via `cx dashboards create`
```

Proceed through the phases in order. Do not jump to Phase 4 before the user has approved the Phase 3 plan, and do not run Phase 7 before Phase 5 and Phase 6 both pass.

---

## Phase 1: Discover telemetry & business meaning

For the target service, gather:

1. **Business purpose** — read the service's `README.md` and top-level entrypoint (`main.*`, `index.*`, `cmd/main.go`, etc.). Summarize in 2–3 sentences what the service does, its key stages, and the critical things that can go wrong.
2. **Metrics** — use the **`metrics-query`** skill rather than grepping manually:
   - For each candidate keyword (service name, subsystem, key verbs like `request`, `error`, `latency`, `dlq`), run `cx metrics search --name '*<keyword>*'`.
   - When a metric looks promising, run `cx metrics get-labels <metric>` to list its labels.
   - **Only use metric names that `cx metrics search` returns.** This is how we prevent invented metrics from reaching Phase 5.
   - Cross-check with the service's instrumentation (`prometheus_client`, `promauto.NewCounter/Histogram/Gauge`, OTel meters, `prom-client`, Micrometer, `metrics.py`) for semantics and histogram buckets. Histograms expose `_sum`, `_count`, `_bucket` suffixes.
3. **Logs** — use the **`query-logs`** skill:
   - Discover custom `$d.*` fields with `cx search-fields "<description>" --dataset logs` before assuming a field exists.
   - Confirm message templates and severity patterns with a small sample: `cx logs 'filter $l.applicationname == "<app>"' --limit 5 -o json`.
   - Standard fields (`$m.severity`, `$m.timestamp`, `$l.applicationname`, `$l.subsystemname`) are always available; no discovery needed.
4. **Spans / traces** — use the **`query-spans`** skill:
   - Discover span attributes with `cx search-fields "<description>" --dataset spans`.
   - Confirm operation and service names with `cx spans 'filter $l.serviceName == "<svc>"' --limit 5 -o json`.
   - Error conventions vary (`$d.tags.error`, `$d.http.status_code`, etc.) — check sample spans before filtering.
5. **Message buses & DLQs** — grep for Kafka, RabbitMQ, SQS, Pub/Sub clients and any `dlq`/`DLQ` references in the service's code. Note topic/queue names so you can add DLQ panels.
6. **Service configuration** — look for infra/deployment config (`meta.yaml`, Helm `values.yaml`, `Deployment`, `Dockerfile`, `chart.yaml`). Extract:
   - The service/application/subsystem name as it appears in Coralogix (the value of `applicationname` / `subsystemname` labels).
   - Tenant/account/team identifiers used as metric or log labels.
   - Deployment environments (`prod`, `staging`, `dev`, etc.).

If the signal source for a question is ambiguous (e.g. "how much revenue last week"), run the **`telemetry-querying`** gateway first to decide whether the answer lives in metrics, logs, or spans.

Produce a short internal summary before moving on. If critical telemetry is missing (e.g. no metrics instrumentation), surface that to the user and ask whether they want log-only or trace-only panels.

---

## Phase 2: Gather dashboard specifications

Before drafting the plan, ask the user a small focused set of questions. Prefer the `AskQuestion` tool when available; otherwise ask inline. Cover:

1. **Audience & use** — on-call triage, product/business tracking, capacity planning, customer success?
2. **Default time range** — typical viewing window (e.g. 24h, 7d). This informs section choices but queries should still use `${__range}` so users can zoom.
3. **Slicing dimensions** — which top-level filters should exist (e.g. `tenant_id`, `account_id`, `subsystem_name`, `region`, `env`)?
4. **Environment scope** — which environments to include/exclude (common default: exclude non-prod like `dev`, `staging`, `test`).
5. **SLO-ish signals** — are there success-rate, latency, or throughput targets the dashboard should highlight?
6. **Priorities** — what's most important to see first? (drives row ordering and which section is `collapsed: true`.)

Keep this to ≤6 questions. Do not block on answers you can reasonably infer — state the inference and continue.

---

## Phase 3: Draft the internal plan

Write a concise markdown plan the user can approve before JSON generation:

```
## Dashboard: <Service> — <Purpose>

### Section 1: <Overview> (collapsed: false)
- Row 1: [widget type] <title> — <what it shows> — query source: metrics|logs|spans
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

**Section design rules**:
- First section = at-a-glance health (gauges + key rates). Keep it always expanded.
- Pair related time-series in the same row (e.g. rate + latency).
- Put raw log tables and rarely-needed breakdowns in a final `collapsed: true` section.
- Aim for 3–5 sections, 6–20 widgets total. Avoid bloat.

**Widget type selection**:

| Signal | Widget type |
|---|---|
| A single "headline" number (count, % success, totals) | `gauge` (this is also what Coralogix calls "stat") |
| Breakdown across ≤8 categories | `pieChart` |
| Change over time (rate, latency, count per bucket) | `lineChart` |
| Top-N tables, last errors, per-entity listings | `dataTable` |

Do not use any other widget type unless explicitly requested.

**Wait for the user to approve or adjust the plan before emitting JSON.**

---

## Phase 4: Generate the Coralogix JSON

Produce a single JSON document matching the structure in [`references/widget-templates.md`](references/widget-templates.md). Key rules:

1. **Top-level shape**:
   ```
   {
     "id": "<random-21-char-id>",
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
2. **IDs** — every `section`, `row`, `widget`, and query `id` needs a UUID. Generate fresh UUIDs; do not reuse ones from examples.
3. **Row height** — use `"appearance": { "height": 19 }` unless there's a reason to change.
4. **Section options** — include `options.custom.name`, `collapsed`, and `color.predefined: "SECTION_PREDEFINED_COLOR_UNSPECIFIED"`.
5. **Filters** — emit one filter entry per slicing dimension from Phase 2. Default operator is `equals` with empty `values` so users can fill in. Use `notEquals` for environment exclusions (see example in [`references/widget-templates.md`](references/widget-templates.md)).
6. **relativeTimeFrame** — default to `"172800s"` (48h) unless the user specified otherwise.

For query syntax, follow [`references/query-syntax.md`](references/query-syntax.md) for the Coralogix-dashboard-specific rules, and delegate to the `dataprime` and `metrics-query` skills for the full language references.

---

## Phase 5: Live-verify every query through the cx CLI

**This phase is mandatory.** Every PromQL and DataPrime query in the draft must successfully parse and run through the `cx` CLI before Phase 7 ships it. This is where invented metric names, typoed field paths, and malformed DataPrime pipelines get caught.

### 1. Resolve the dashboard time range

Parse `relativeTimeFrame` from the draft (default `"172800s"` = 48h) into a human token and call it `$RANGE`:

| `relativeTimeFrame` | `$RANGE` token |
|---|---|
| 3600s | `1h` |
| 21600s | `6h` |
| 86400s | `24h` |
| 172800s | `48h` |
| 604800s | `7d` |

Every verification query uses `$RANGE` so the CLI check matches what the dashboard will actually evaluate post-import. **Never substitute a hard-coded `[5m]`** — that would test a different window than the dashboard runs.

### 2. Verify each PromQL query

For every widget whose definition contains a `promqlQuery`, substitute `${__range}` in the expression with `[$RANGE]` (e.g. `[48h]`). Leave any other fixed window (`[5m]`, `[1h]`) untouched — those were placed intentionally for sliding-rate panels.

- **Instant-style widgets** (`gauge` / `pieChart` / `dataTable` with `promqlQueryType: PROM_QL_QUERY_TYPE_INSTANT`):
  ```bash
  cx metrics query '<expression-with-[$RANGE]-substituted>' -o agents
  ```
- **Time-series widgets** (`lineChart`):
  ```bash
  cx metrics query-range '<expression>' --start now-$RANGE --end now --step <auto> -o agents
  ```
  Pick `<step>` proportional to `$RANGE`: `1m` for 1–6h, `5m` for 24h, `1h` for 7d+. Match any window used by a `*_over_time` / `rate` / `increase` inside the expression if it's narrower than `$RANGE`.

A query **passes** when the CLI returns a 200 response and either has data or an empty-but-well-formed result. **Fails** include: unknown metric names, parse errors, non-200 responses, or `cx` error output.

On failure: consult the `metrics-query` skill for PromQL help, re-search for the real metric name with `cx metrics search`, re-list labels with `cx metrics get-labels`, and fix the query in the draft JSON. Budget **≤5 retry attempts per query**.

### 3. Verify each DataPrime query

For every widget whose definition contains a `dataprimeQuery`, run the query against the right source:

- Log-backed widgets:
  ```bash
  cx logs '<query-without-source-prefix>' --start now-$RANGE --end now --limit 1
  ```
- Span-backed widgets:
  ```bash
  cx spans '<query-without-source-prefix>' --start now-$RANGE --end now --limit 1
  ```

A query **passes** when the CLI returns without a parse error. Empty results are acceptable. On failure: consult the `dataprime` skill (`cx dataprime show <command>` for inline help), re-discover fields with `cx search-fields`, fix, retry. Budget **≤5 retry attempts per query**.

### 4. Restore `${__range}` before Phase 6

Once every PromQL and DataPrime query passes, restore `${__range}` (and any other variables) in the emitted JSON. **The verification step uses the concrete `$RANGE`; the final JSON keeps the injected variables intact.**

If any query cannot be made to pass within the retry budget, surface it to the user with the CLI error verbatim — do not silently ship a broken widget.

---

## Phase 6: Self-verify structure (mandatory before deploy)

Run through this checklist against the final JSON. **If any item fails, fix and re-check before Phase 7.**

### Query syntax (Coralogix-dashboard-specific)
- [ ] Every PromQL range vector inside a metrics widget uses `[${__range}]` — never `[$__range]`, never `[5m]` (unless intentionally a sliding window).
- [ ] `promqlQueryType` is `PROM_QL_QUERY_TYPE_INSTANT` for single-value widgets (gauge, pieChart, dataTable). Time-series widgets (lineChart) omit it.
- [ ] DataPrime log queries use `$d.message` / `$l.applicationname` / unquoted severity enums (full syntax rules: the `dataprime` skill reference).
- [ ] Success-rate denominators wrapped in `clamp_min(..., 1)`.
- [ ] Histogram queries use the correct suffix (`_sum`, `_count`, `_bucket`).

### Structure
- [ ] Each section has `id.value`, `rows`, and `options.custom`.
- [ ] Each row has `id.value`, `appearance.height`, and `widgets`.
- [ ] Each widget has a unique `id.value` and a `definition` with exactly one of `gauge` / `pieChart` / `lineChart` / `dataTable`.
- [ ] Gauges that represent success-rate use `thresholdType: "THRESHOLD_TYPE_ABSOLUTE"` with green at high values; gauges for error/DLQ counts use thresholds with red at high values.
- [ ] "Total" / "stat" style widgets are encoded as `gauge`, not as a stat type.
- [ ] Top-level `filters` array includes each slicing dimension the user approved.
- [ ] All IDs are freshly generated UUIDs, unique within the document.

### Content
- [ ] Dashboard name is descriptive, e.g. `"<Service> — <Purpose>"`.
- [ ] Widget titles are short, human-readable, and match what the query actually computes.
- [ ] The last section (logs/errors) is `collapsed: true` unless the user said otherwise.

---

## Phase 7: Deploy via `cx dashboards create`

Do **not** tell the user to paste JSON into the Coralogix UI. Deploy the dashboard directly.

### 1. Pick a folder

List folders and suggest the best match:

```bash
cx dashboards folders list -o json
```

Offer the user the closest folder by name/keyword (e.g. the service's team, product area, or a folder literally named after the service). Default to "root" (no `--folder`) if no folder fits. If the user wants a new folder, ask them to create it in the Coralogix UI and then rerun — folder creation is outside this skill's scope.

Present the choices with `AskQuestion` when available:

- "Folder X (id: `<id>`) — best match by name"
- "Folder Y (id: `<id>`)"
- "Root (no folder)"
- "Create a new folder first (I'll pause)"

### 2. Save and deploy

Write the verified JSON to a temp file and create:

```bash
# Write the JSON to a reproducible path
cat > /tmp/cx-dashboard-<slug>.json <<'JSON'
{ ...full verified dashboard... }
JSON

# Deploy into the chosen folder (omit --folder for root)
cx dashboards create --from-file /tmp/cx-dashboard-<slug>.json --folder <folder-id>
```

The CLI generates the `requestId` envelope automatically and prints the created dashboard ID and name on success. Pipe into `-o json` or `-o agents` if you need structured output.

**On failure**: show the CLI error verbatim, return to Phase 5 (most common cause: a query that parses locally but the live API rejects), fix, and redeploy.

**On success**: emit the summary below.

### 3. Idempotency note

Each run generates a fresh top-level `id` (21-char nanoid), so re-running this skill creates a *new* dashboard rather than overwriting an existing one. If the user wants to replace an existing dashboard, point them at the Coralogix API's "replace dashboard" endpoint — that's outside this skill's current scope.

---

## Output format for the user

````
## Plan
<the approved Phase 3 plan>

## Verification
- PromQL queries verified: <N>/<N>
- DataPrime queries verified: <N>/<N>

## Deployed
- Dashboard: **<Name>**
- ID: `<id>`
- Folder: `<folder name or "root">`
- Profile: `<cx profile>`

The dashboard is live in Coralogix now. Adjust filter values (e.g. `account_id`) after opening it.
````

---

## References

- Coralogix dashboard-specific query gotchas (`${__range}`, `promqlQueryType`, widget filters): [`references/query-syntax.md`](references/query-syntax.md)
- Widget JSON templates (copy & adapt): [`references/widget-templates.md`](references/widget-templates.md)
- Full DataPrime language: `dataprime` skill → `skills/dataprime/references/dataprime-reference.md`
- Full PromQL reference: `metrics-query` skill → `skills/metrics-query/references/promql-guidelines.md`
- Inline DataPrime help: `cx dataprime list`, `cx dataprime show <command>`
