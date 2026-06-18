# Events2Metrics (E2M) Schema Reference

Complete JSON schema reference for Coralogix Events2Metrics definitions, using the **actual REST API wire format** that `cx e2m` reads and writes. Use this when constructing payloads for `cx e2m create` / `cx e2m update`.

> **Tip:** The most reliable way to build an E2M is to fetch an existing one with `cx e2m get <id> -o json`, strip the read-only fields, modify, and pipe it into `cx e2m create --from-file -`. Note that `cx e2m list` and `cx e2m create` print a *simplified* view (id/name/type/metric_name) — **only `cx e2m get` returns the full definition** you can template from.

## How E2M is computed (read this first)

E2M is **not** a stored query — it aggregates events **as they stream through the real-time ingestion pipeline** into metric series (~1-minute resolution). It is **forward-only**: metrics start accumulating from the moment the E2M is created; there is no backfill.

- All ingested data flows through the ingestion pipeline; a **TCO policy** routes each stream into a tier.
- E2M works on data in **High tier** (Frequent Search / hot storage) **and Medium tier** (lands in S3 archive, but still processed by the pipeline).
- E2M does **NOT** work on **Low / compliance tier** (compliance storage only, no aggregation features) or blocked data.
- The axis is **tier / processing level — not "Frequent Search vs archive"** (Medium *is* archive and E2M works on it).

If an E2M produces **zero metric series**, the usual cause is that its source data is routed to Low/compliance (or the query matches nothing) — the fix is a TCO change, not an E2M change. See the SKILL workflow's troubleshooting section.

---

## Source types

The `type` field + `query` oneof select what events are aggregated:

| `type` (string enum) | `query` field | Source |
|---|---|---|
| `E2M_TYPE_LOGS2METRICS` | `logsQuery` | Logs |
| `E2M_TYPE_SPANS2METRICS` | `spansQuery` | Spans |

`E2M_TYPE_UNSPECIFIED` (0) is never used in a real definition.

**Datasets (`dataSource`):** an optional top-level `dataSource` string in `"<dataspace>/<dataset>"` format (e.g. `"my_dataspace/my_dataset"`) scopes the E2M to a specific dataset instead of the standard logs/spans stream. It is supported via the API/CLI — add it to the create body. If omitted, the E2M defaults to the standard logs/spans stream for the chosen `type`.

> **Requires an account feature flag.** `dataSource` is gated behind `e2m_dataset_source_enabled` per company. If the feature isn't enabled, the API rejects the definition with `"dataSource is not enabled for this company"`. If you hit that error, the account needs the feature turned on — it's not a payload problem.

```json
{ "type": "E2M_TYPE_LOGS2METRICS", "dataSource": "my_dataspace/my_dataset", "logsQuery": { ... } }
```

---

## Top-level structure

`cx e2m create` and `cx e2m update` post the **bare definition object** (the HTTP body maps to the `e2m` field). A `cx e2m get` *response* wraps the same object as `{"e2m": { ... }}`, so when templating, extract `.e2m`.

**Creating a new E2M** — drop `id` (and other read-only fields); the server assigns a fresh id:

```bash
cx e2m get <id> -o json | jq '.e2m | del(.id, .permutations, .createTime, .updateTime, .metricName)' > e2m.json
cx e2m create --from-file e2m.json
```

**Updating an existing E2M** — `cx e2m update` is a PUT with **no id in the path**, so the body **must keep `id`** (it's the only identifier the API uses). Drop only the server-derived fields:

```bash
cx e2m get <id> -o json | jq '.e2m | del(.permutations, .createTime, .updateTime, .metricName)' > e2m.json
# edit e2m.json (keep "id"), then:
cx e2m update --from-file e2m.json
```

### Create payload (`E2MCreateParams`)

```json
{
  "name": "service_catalog_latency",
  "description": "avg + sum latency for catalog service",
  "permutationsLimit": 30000,
  "type": "E2M_TYPE_LOGS2METRICS",
  "logsQuery": {
    "lucene": "coralogix.metadata.applicationName:catalog",
    "applicationnameFilters": [],
    "subsystemnameFilters": [],
    "severityFilters": ["SEVERITY_ERROR", "SEVERITY_CRITICAL"]
  },
  "metricLabels": [
    { "targetLabel": "app",       "sourceField": "coralogix.metadata.applicationName" },
    { "targetLabel": "subsystem", "sourceField": "coralogix.metadata.subsystemName" }
  ],
  "metricFields": [
    {
      "targetBaseMetricName": "latency_ms",
      "sourceField": "log_obj.latency_ms",
      "aggregations": [
        { "enabled": true, "aggType": "AGG_TYPE_COUNT", "targetMetricName": "latency_ms_count" },
        { "enabled": true, "aggType": "AGG_TYPE_AVG",   "targetMetricName": "latency_ms_avg" },
        { "enabled": true, "aggType": "AGG_TYPE_SUM",   "targetMetricName": "latency_ms_sum" }
      ]
    }
  ]
}
```

### Field reference

| Field | Type | Notes |
|---|---|---|
| `name` | string | **Required.** |
| `description` | string | Optional. |
| `permutationsLimit` | int | Create-only. Caps the label permutation cardinality (e.g. `30000`). |
| `type` | enum string | `E2M_TYPE_LOGS2METRICS` or `E2M_TYPE_SPANS2METRICS`. Required. |
| `dataSource` | string | Optional. `"<dataspace>/<dataset>"` to scope to a dataset; omit for the standard logs/spans stream. |
| `logsQuery` / `spansQuery` | object | The `query` oneof — exactly one, matching `type`. |
| `metricLabels[]` | array | Each label becomes a Prometheus label. **Each distinct value set multiplies permutations.** |
| `metricFields[]` | array | **Max 10.** Each holds a source field + its aggregations. |

**`id`** — UUID. **Omit on `create`** (server assigns it); **required in the body on `update`** (the PUT has no path id). Present in `get` responses.

**Server-derived fields** (present in `get` responses; drop from create/update bodies): `permutations` (`{limit, hasExceededLimit}`), `createTime`, `updateTime`, `metricName`, `isInternal`.

### `logsQuery`

| Field | Type |
|---|---|
| `lucene` | string (Lucene filter) |
| `alias` | string (optional) |
| `applicationnameFilters[]` | string[] |
| `subsystemnameFilters[]` | string[] |
| `severityFilters[]` | enum string[] — see below |

Severity enum values: `SEVERITY_DEBUG`, `SEVERITY_VERBOSE`, `SEVERITY_INFO`, `SEVERITY_WARNING`, `SEVERITY_ERROR`, `SEVERITY_CRITICAL`.

### `spansQuery`

| Field | Type |
|---|---|
| `lucene` | string (Lucene filter) |
| `applicationnameFilters[]` | string[] |
| `subsystemnameFilters[]` | string[] |
| `actionFilters[]` | string[] |
| `serviceFilters[]` | string[] |

### `metricLabels[]`

```json
{ "targetLabel": "app", "sourceField": "coralogix.metadata.applicationName" }
```

- `targetLabel` — the output Prometheus label name. Pattern `^[\w/-]+$`.
- `sourceField` — the event field to read the value from (e.g. `coralogix.metadata.applicationName`, `log_obj.region`).

### `metricFields[]` and `aggregations[]`

```json
{
  "targetBaseMetricName": "latency_ms",
  "sourceField": "log_obj.latency_ms",
  "aggregations": [
    { "enabled": true, "aggType": "AGG_TYPE_HISTOGRAM", "targetMetricName": "latency_ms_histogram",
      "histogram": { "buckets": [10, 50, 100, 500, 1000] } },
    { "enabled": true, "aggType": "AGG_TYPE_SAMPLES",   "targetMetricName": "latency_ms_max",
      "samples": { "sampleType": "SAMPLE_TYPE_MAX" } }
  ]
}
```

- `targetBaseMetricName` — base name; each aggregation gets its own `targetMetricName`. Pattern `^[\w/-]+$`.
- `sourceField` — numeric event field to aggregate. (For `AGG_TYPE_COUNT` the value is irrelevant — it counts matching events.)

**`aggType` values:**

| `aggType` (string enum) | Meaning | Extra metadata |
|---|---|---|
| `AGG_TYPE_MIN` | minimum | - |
| `AGG_TYPE_MAX` | maximum | - |
| `AGG_TYPE_COUNT` | event count | - |
| `AGG_TYPE_AVG` | average | - |
| `AGG_TYPE_SUM` | sum | - |
| `AGG_TYPE_HISTOGRAM` | bucketed distribution | `histogram.buckets`: `float[]` (bucket boundaries; ≥1 bucket) |
| `AGG_TYPE_SAMPLES` | sampled min/max | `samples.sampleType`: `SAMPLE_TYPE_MIN` or `SAMPLE_TYPE_MAX` |

Each aggregation: `enabled` (bool), `aggType` (string enum), `targetMetricName` (string), plus the `agg_metadata` oneof (`histogram` or `samples`) only for those two types.

---

## Cardinality & Limits

**Permutations = the product of the number of distinct values across all `metricLabels`.** A label on `app` (20 values) × `subsystem` (15) × `status_code` (40) = 12,000 permutations. Add one high-cardinality label and it explodes.

- **Never** use high-cardinality fields as labels: user IDs, request/trace IDs, session IDs, raw URLs, full paths, IPs, timestamps. These multiply permutations without bound and will breach the limit.
- Keep labels to bounded, low-cardinality dimensions (app, subsystem, region, status class, severity, endpoint *group*).
- `permutationsLimit` caps the design; if exceeded, the read-back `permutations.hasExceededLimit` is `true` and new permutations stop being produced.

**Checking limits and forecasting cardinality:**

```bash
cx e2m limits -o json              # account E2M count limit + used
cx e2m labels-cardinality -o json  # see caveat below
```

- `cx e2m limits` reports the E2M **count** limit and how many are used (the underlying API also tracks labels/permutations/metrics caps).
- **The labels-cardinality endpoint is a draft forecast tool.** Given a proposed query + `metricLabels`, the backend runs a cardinality aggregation over the **last 7 days** of the company's logs/spans and returns the distinct label-permutation count **per day** for exactly that draft design — so you can size a design *before* creating it. With no labels supplied it returns an empty result.
- **CLI limitation:** `cx e2m labels-cardinality` currently takes **no arguments**, so it sends no draft and returns an **empty list** — it does not expose the forecast yet (it would need `--metric-labels`/`--query`). Until then, forecast a draft via the **Coralogix UI** (the create flow calls this endpoint with your labels+query), or estimate manually (below) as a rough check.
- **The forecast only sees Frequent-Search (High-tier) data** — it queries the hot `*_newlogs*` index. For a Medium-tier (archive) source it will undercount, so treat manual estimates as the floor.

**Manual estimate (rough fallback) — sizing a 2-label design:**
- Labels: `app` (~12 distinct), `severity` (6 distinct).
- Estimated permutations = 12 × 6 = **72** — comfortably under any limit.
- Adding `endpoint` (~300 distinct) → 12 × 6 × 300 = **21,600**. Closer to limits; prefer grouping endpoints into a bounded `route` field, or drop the label and filter in `lucene` instead.

---

## Gotchas

- **Enums are strings, not integers.** Use `"type": "E2M_TYPE_LOGS2METRICS"` and `"aggType": "AGG_TYPE_AVG"` — not `1` / `4`. (Some internal/legacy exports use integers and a `$case` discriminator; the `cx` CLI uses the OpenAPI string form.)
- **Template from `get`, not `list`.** `cx e2m list` / `create` print a simplified summary; only `cx e2m get <id> -o json` returns the full definition (`{"e2m": {...}}`).
- **Create/update body is the bare object**, not wrapped in `{"e2m": ...}`. Extract `.e2m` from a `get` response first.
- **`update` (replace) requires `id` in the body** — the PUT has no path id, so dropping `id` makes the update fail to find the rule. `create` must omit `id`.
- **Simple aggregations (MIN/MAX/COUNT/AVG/SUM) need no `agg_metadata`** — only `histogram` and `samples` carry metadata. (The proto models the empty case as a `none` marker; in JSON, just omit the metadata.)
- **Max 10 metric fields** per E2M.
- **`targetMetricName` must be unique** within the definition; collisions silently clobber series.
- **Forward-only.** Creating an E2M never backfills historical data — only events ingested after creation produce series.
- **Verify with metrics, not logs:** `cx metrics search --name "<targetBaseMetricName>"` then a PromQL query.
