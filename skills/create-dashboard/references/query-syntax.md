# Coralogix Dashboard Query Syntax — Dashboard-Specific Gotchas

This file documents **only** the non-obvious rules that are specific to authoring a Coralogix dashboard JSON — the things that silently break an imported dashboard even when the query itself is valid.

For the underlying query languages, use the sibling skills:

- **DataPrime** (filters, aggregations, operators, type conversions, `extract`, `roundTime`): `dataprime` skill → `skills/dataprime/references/dataprime-reference.md`.
- **PromQL** (counters vs gauges, histograms, label matching, temporal reductions, full function list): `metrics-query` skill → `skills/metrics-query/references/promql-guidelines.md`.
- **Inline help**: `cx dataprime list` and `cx dataprime show <command>`.

---

## 1. The `${__range}` dashboard variable

Coralogix injects the dashboard-level time range as `${__range}` — this is a dashboard-only variable, **not** a PromQL feature.

- Correct: `increase(foo_total[${__range}])`
- Wrong: `increase(foo_total[$__range])` — missing braces, Coralogix drops it
- Wrong: `increase(foo_total[5m])` — hard-codes a 5-minute window; the dashboard time picker has no effect

Use fixed ranges (`[5m]`, `[1h]`) **only** when the panel intentionally shows a rolling window independent of the dashboard time picker (rare; document it in the panel title).

### Verifying `${__range}` queries with the CLI

`cx metrics query` does not expand `${__range}` (it's a dashboard-side substitution). During Phase 5 verification, swap `${__range}` for the concrete token that matches the dashboard's `relativeTimeFrame` (e.g. `[48h]` for the default `"172800s"`). Restore `${__range}` in the JSON before deploy.

---

## 2. `promqlQueryType` — instant vs time-series widgets

Every metrics widget has a `promqlQueryType`:

- `PROM_QL_QUERY_TYPE_INSTANT` — single point in time. **Required** for `gauge`, `pieChart`, and `dataTable`.
- Omit (or default) — time series. Use for `lineChart`.

Leaving an instant widget in time-series mode makes the panel render a single average across the window instead of the intended single-point value, which breaks success-rate gauges and top-N tables.

---

## 3. DataPrime inside widget JSON

The DataPrime language itself is documented in the `dataprime` skill. A few rules matter specifically inside dashboard widgets:

- The dashboard always supplies a source, so widget queries **should not** start with `source logs` or `source spans` — use the bare pipeline: `filter $m.severity == ERROR | agg count()`.
- The JSON key is `dataprimeQuery.text` (a string), **not** `dataprimeQuery.value`.
- `filter` / `groupby` / aggregate forms use the same syntax as the CLI — the `dataprime` skill is authoritative.
- Severity enums are unquoted in DataPrime: `$m.severity == ERROR`, not `"ERROR"`.

### Widget-side gotchas that commonly appear in reviews

- `$d.message`, not `$m.text`.
- `contains` is a method on the field: `$d.message.contains('timeout')`, not `$d.message contains 'timeout'`.
- Application filter uses `$l.applicationname` (lowercase), not `$m.applicationName`.

If you're unsure, consult the `dataprime` skill reference rather than guessing.

---

## 4. PromQL idioms that recur in dashboards

The full PromQL reference is in the `metrics-query` skill. The patterns below show up in almost every dashboard and are the ones to copy-paste.

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

**Success rate (%) with safe denominator** (always wrap denominators in `clamp_min(..., 1)`):
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

Only use if the user explicitly requests Lucene — prefer DataPrime.

- Severity: `coralogix.metadata.severity:ERROR`
- Application: `coralogix.metadata.applicationName:"my-service"`
- Field match: `message:"is stuck"`

---

## 6. Dashboard-syntax checklist (apply before Phase 5)

- [ ] PromQL range vectors inside widgets use `[${__range}]`.
- [ ] `promqlQueryType` is `PROM_QL_QUERY_TYPE_INSTANT` for `gauge` / `pieChart` / `dataTable`; omitted for `lineChart`.
- [ ] DataPrime queries in widgets don't start with `source logs` / `source spans` (the dashboard provides the source).
- [ ] DataPrime `contains` is written as `.contains(...)`.
- [ ] DataPrime severity enums are unquoted (`ERROR`, `CRITICAL`, ...).
- [ ] Success-rate denominators wrapped in `clamp_min(..., 1)`.
- [ ] Histogram queries use the correct suffix (`_sum`, `_count`, `_bucket`).
- [ ] No invented metric names — every PromQL metric appeared in `cx metrics search` during Phase 1.
