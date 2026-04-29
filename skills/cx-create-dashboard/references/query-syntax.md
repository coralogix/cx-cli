# Coralogix Dashboard Query Syntax - Dashboard-Specific Gotchas

This file documents **only** the non-obvious rules specific to authoring a Coralogix dashboard JSON - the things that silently break an imported dashboard even when the query itself is valid.

For the underlying query languages, use the sibling skills:

- **DataPrime** (filters, aggregations, operators, type conversions, `extract`, `roundTime`): `cx-dataprime` skill → `skills/cx-dataprime/references/dataprime-reference.md`.
- **PromQL** (counters vs gauges, histograms, label matching, temporal reductions, full function list): `cx-metrics-query` skill → `skills/cx-metrics-query/references/promql-guidelines.md`.
- **Inline help**: `cx dataprime list` and `cx dataprime show <command>`.

---

## 1. The `${__range}` dashboard variable

Coralogix injects the dashboard-level time range as `${__range}` - this is a dashboard-only variable, not a PromQL feature.

- Correct: `increase(foo_total[${__range}])`
- Wrong: `increase(foo_total[$__range])` - missing braces; Coralogix drops it
- Wrong: `increase(foo_total[5m])` - hard-codes a 5-minute window; the dashboard time picker has no effect

Use fixed ranges (`[5m]`, `[1h]`) only when the panel intentionally shows a rolling window independent of the dashboard time picker (rare; document it in the panel title).

### Verifying `${__range}` queries with the CLI

`cx metrics query` doesn't expand `${__range}` (it's a dashboard-side substitution). During Phase 5 verification, swap `${__range}` for the concrete token that matches the dashboard's `relativeTimeFrame` (e.g. `[48h]` for the default `"172800s"`). Restore `${__range}` in the JSON before deploy.

---

## 2. `promqlQueryType` - instant vs time-series widgets

Every metrics widget has a `promqlQueryType`:

- `PROM_QL_QUERY_TYPE_INSTANT` - single point in time. Required for `gauge`, `pieChart`, and `dataTable`.
- Omit (or default) - time series. Use for `lineChart`.

Leaving an instant widget in time-series mode makes the panel render a single average across the window instead of the intended single-point value, which breaks success-rate gauges and top-N tables.

---

## 3. DataPrime inside widget JSON

The DataPrime language itself is documented in the `cx-dataprime` skill. A few rules matter specifically inside dashboard widgets:

- DataPrime widget queries **must** start with `source logs` or `source spans` - Coralogix doesn't infer the source for DataPrime widgets the way it does for metrics. Example: `source logs | filter $m.severity == ERROR | agg count()`.
- The JSON key is `dataprimeQuery.text` (a string), not `dataprimeQuery.value`.
- `filter` / `groupby` / aggregate forms use the same syntax as the CLI - the `cx-dataprime` skill is authoritative.
- Severity enums are unquoted in DataPrime: `$m.severity == ERROR`, not `"ERROR"`.

### Widget-side gotchas that commonly appear in reviews

- `$d.message`, not `$m.text`.
- `contains` is a method on the field: `$d.message.contains('timeout')`, not `$d.message contains 'timeout'`.
- Application filter uses `$l.applicationname` (lowercase), not `$m.applicationName`.

If you're unsure, consult the `cx-dataprime` skill reference rather than guessing.

---

## 4. PromQL idioms that recur in dashboards

The full PromQL reference is in the `cx-metrics-query` skill. The patterns below show up in almost every dashboard and are the ones to copy-paste.

**Histogram average over the dashboard range**:

```
sum by (label) (rate(foo_latency_sum[${__range}]))
/
sum by (label) (rate(foo_latency_count[${__range}]))
```

**Histogram P95**:

```
histogram_quantile(0.95,
  sum by (le, label) (rate(foo_latency_bucket[${__range}]))
)
```

**Counter increments over the range**:

```
sum by (account_id) (increase(foo_total[${__range}]))
```

**Success rate (%) with safe denominator** (wrap denominators in `clamp_min(..., 1)`):

```
100 *
  sum(increase(foo_success_total[${__range}]))
  /
  clamp_min(
    sum(increase(foo_success_total[${__range}]))
    + sum(increase(foo_failure_total[${__range}])),
    1
  )
```

**Propagating a label across metrics** (metric A has `account_id`; metric B has both `account_id` and `account_slug`):

```
sum by (account_id) (increase(foo_total[${__range}]))
* on (account_id) group_left(account_slug)
max by (account_id, account_slug) (
  (max_over_time(bar_total{account_slug!=""}[365d] @ end()) * 0) + 1
)
```

---

## 5. Lucene (legacy logs query)

Only use if the user explicitly requests Lucene - prefer DataPrime.

- Severity: `coralogix.metadata.severity:ERROR`
- Application: `coralogix.metadata.applicationName:"my-service"`
- Field match: `message:"is stuck"`

---

## 6. Cross-references: how Olly and Coralogix consume this JSON

Queries authored by this skill are eventually consumed by the Coralogix dashboard-rendering layer and by the Olly agent that analyses dashboards. The rules below reflect how the JSON is actually treated - verified against the `cx-olly`, `olly`, and `olly-knowledge-base` repositories.

### Field precedence Olly enforces

Source: `olly/apps/api/src/api/agent/handlers/utils/dashboard_enrichment.py` (`_get_important_fields`) surfaces exactly two dashboard fields to the LLM as "important":

- **`relativeTimeFrame`** - Olly uses this as the default time range when the user doesn't pass `from`/`to`. Our Phase 5 CLI verification therefore has to run with the same `relativeTimeFrame` - don't verify a 48h dashboard with a `[5m]` window.
- **`filters`** - Olly injects the dashboard-level `filters` array into every DataPrime and PromQL query before sending it. `DASHBOARD_FOCUSED_PROMPT` in `olly/apps/api/src/api/agent/agents/orchestrator_agent/dashboard_prompts.py` instructs: *"EDIT ALL queries to INCLUDE THE FILTERS FROM THE DASHBOARD DATA"*. Consequence: widget queries must be valid **before** the dashboard filters are injected - do not pre-bake the slicing dimensions into the widget's query text.

### JSON keys Olly consumes

Source: `WHITELIST_DASHBOARD_KEYS` in `olly/apps/api/src/api/agent/handlers/utils/dashboard_enrichment.py`:

```
name, description, title, isVisible, columns, query, unit, scaleType,
variables, variablesV2, filters, relativeTimeFrame, annotations,
twoMinutes, slugName, actions, updatedAt, createdAt, updaterAuthorId,
updaterName, authorId, authorName, updatedOriginType, createdOriginType
```

Prefer these exact field names over synonyms. Keys outside this list are dropped from Olly's view of the dashboard.

### Allowed query languages in a widget

Source: `olly-knowledge-base/common/src/common/models/dashboard_context_models.py` (`QueryEmbedding.query_type`): only `dataprime`, `promql`, or `lucene`. Nothing else will be indexed or embedded downstream.

### Sibling Olly skill

The dashboard-analysis counterpart to this skill lives at `cx-olly/apps/api/src/api/agent/agents/skills_agent/skills/dashboards.md`. It covers the consume-side rules (accept dashboard links, honor `var-*` parameters, always add `filters` + `relativeTimeFrame` to derived queries). Our authoring rules above mirror its expectations.

### Coralogix docs

- Overview: <https://www.coralogix.com/docs/user-guides/custom-dashboards/introduction/>
- Widgets index: <https://www.coralogix.com/docs/user-guides/custom-dashboards/widgets/>
- Query builder: <https://www.coralogix.com/docs/user-guides/custom-dashboards/tutorials/query-builder/>
- Dashboard settings (slugs, folders): <https://www.coralogix.com/docs/user-guides/custom-dashboards/tutorials/manage-dashboard-settings/>

---

## 7. Dashboard-syntax checklist (apply before Phase 5)

- [ ] PromQL range vectors inside widgets use `[${__range}]`.
- [ ] `promqlQueryType` is `PROM_QL_QUERY_TYPE_INSTANT` for `gauge` / `pieChart` / `dataTable`; omitted for `lineChart`.
- [ ] DataPrime widget queries start with `source logs` or `source spans` (required for dashboard widgets; stripped during Phase 5 CLI verification).
- [ ] DataPrime `contains` is written as `.contains(...)`.
- [ ] DataPrime severity enums are unquoted (`ERROR`, `CRITICAL`, …).
- [ ] Success-rate denominators wrapped in `clamp_min(..., 1)`.
- [ ] Histogram queries use the correct suffix (`_sum`, `_count`, `_bucket`).
- [ ] No invented metric names - every PromQL metric appeared in `cx metrics search` during Phase 1.
- [ ] Widget queries remain valid **without** the dashboard-level `filters` (Coralogix/Olly inject them at render time).
- [ ] Widget query language is one of `dataprime`, `promql`, or `lucene` - nothing else.
- [ ] `relativeTimeFrame` in the final JSON matches the window used for Phase 5 verification.
