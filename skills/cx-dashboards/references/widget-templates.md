# Coralogix Dashboard JSON - Widget Templates

Copy these templates when generating a dashboard. **Always replace every UUID** (`id.value`, query `id`) with a freshly generated UUID, and adapt queries to the target service.

For query-language rules:

- Dashboard-specific gotchas (`${__range}`, `promqlQueryType`, widget filters): [`query-syntax.md`](query-syntax.md).
- Full DataPrime syntax: `cx-dataprime` skill → `skills/cx-dataprime/references/dataprime-reference.md`.
- Full PromQL reference: `cx-metrics-query` skill → `skills/cx-metrics-query/references/promql-guidelines.md`.

> A "stat" widget in Coralogix docs is actually emitted as `gauge` in the JSON. There is no separate stat type.

> Every query below must pass live verification via `cx metrics query` / `cx logs` / `cx spans` before deploy - see SKILL.md Phase 5.

---

## Top-level skeleton

```json
{
  "id": "<21-char-nanoid>",
  "name": "<Service> - <Purpose>",
  "layout": {
    "sections": [
      { "id": {"value": "<uuid>"}, "rows": [ ... ], "options": { "custom": { "name": "Section name", "collapsed": false, "color": {"predefined": "SECTION_PREDEFINED_COLOR_UNSPECIFIED"} } } }
    ]
  },
  "variables": [],
  "variablesV2": [],
  "filters": [ /* see "Top-level filters" below */ ],
  "relativeTimeFrame": "172800s",
  "annotations": [],
  "off": {},
  "actions": []
}
```

A row always looks like:

```json
{
  "id": {"value": "<uuid>"},
  "appearance": {"height": 19},
  "widgets": [ /* 1–2 widgets per row */ ]
}
```

---

## Widget: gauge (also used for "stat"/total)

Use for headline numbers: counts, percentages, success rates.

Gauge `min` and `max` must be numeric and `min < max`. This API constraint applies even when `thresholdType` is `THRESHOLD_TYPE_ABSOLUTE`; use a real range such as `0..100` for percentages, a known capacity/limit when available, or a max above the highest threshold.

```json
{
  "id": {"value": "<uuid>"},
  "title": "Success Rate",
  "definition": {
    "gauge": {
      "query": {
        "metrics": {
          "promqlQuery": {
            "value": "100 * sum(increase(foo_success_total[${__range}])) / clamp_min(sum(increase(foo_success_total[${__range}])) + sum(increase(foo_failure_total[${__range}])), 1)"
          },
          "aggregation": "AGGREGATION_UNSPECIFIED",
          "filters": [],
          "editorMode": "METRICS_QUERY_EDITOR_MODE_TEXT",
          "promqlQueryType": "PROM_QL_QUERY_TYPE_INSTANT"
        }
      },
      "min": 0,
      "max": 100,
      "showInnerArc": true,
      "showOuterArc": true,
      "unit": "UNIT_PERCENT",
      "thresholds": [
        {"from": 0,  "color": "var(--c-visualization-red-05)"},
        {"from": 80, "color": "var(--c-visualization-yellow-05)"},
        {"from": 95, "color": "var(--c-visualization-green-05)"}
      ],
      "dataModeType": "DATA_MODE_TYPE_HIGH_UNSPECIFIED",
      "thresholdBy": "THRESHOLD_BY_UNSPECIFIED",
      "decimal": 2,
      "thresholdType": "THRESHOLD_TYPE_ABSOLUTE",
      "legend": {"isVisible": true, "columns": [], "groupByQuery": true, "placement": "LEGEND_PLACEMENT_AUTO"},
      "legendBy": "LEGEND_BY_GROUPS",
      "displaySeriesName": false,
      "decimalPrecision": false
    }
  }
}
```

**For "count of bad things" (errors, DLQ):** use `unit: "UNIT_NUMBER"`, green at low values, red at high:

```json
"thresholds": [
  {"from": 0,  "color": "var(--c-visualization-green-05)"},
  {"from": 1,  "color": "var(--c-visualization-yellow-05)"},
  {"from": 10, "color": "var(--c-visualization-red-05)"}
],
"thresholdType": "THRESHOLD_TYPE_ABSOLUTE"
```

**For DataPrime-driven count (e.g. error log count):** swap `metrics.promqlQuery` for `dataprime.dataprimeQuery`:

```json
"query": {
  "dataprime": {
    "dataprimeQuery": {"text": "source logs | filter $m.severity == ERROR || $m.severity == CRITICAL | agg count()"},
    "filters": []
  }
}
```

---

## Widget: pieChart

Use for small-cardinality breakdowns (≤8 slices).

```json
{
  "id": {"value": "<uuid>"},
  "title": "Messages Per Env",
  "definition": {
    "pieChart": {
      "query": {
        "metrics": {
          "promqlQuery": {"value": "sum by (subsystem_name) (increase(foo_total[${__range}]))"},
          "filters": [],
          "groupNames": ["subsystem_name"],
          "editorMode": "METRICS_QUERY_EDITOR_MODE_TEXT",
          "promqlQueryType": "PROM_QL_QUERY_TYPE_INSTANT",
          "aggregation": "AGGREGATION_UNSPECIFIED"
        }
      },
      "maxSlicesPerChart": 8,
      "minSlicePercentage": 1,
      "stackDefinition": {"maxSlicesPerStack": 4},
      "labelDefinition": {"labelSource": "LABEL_SOURCE_INNER", "isVisible": true, "showName": true, "showValue": true, "showPercentage": true},
      "showLegend": true,
      "unit": "UNIT_UNSPECIFIED",
      "colorScheme": "classic",
      "dataModeType": "DATA_MODE_TYPE_HIGH_UNSPECIFIED",
      "decimal": 2,
      "legend": {"isVisible": true, "columns": [], "groupByQuery": true, "placement": "LEGEND_PLACEMENT_AUTO"},
      "hashColors": false,
      "decimalPrecision": false,
      "showTotal": false
    }
  }
}
```

---

## Widget: lineChart

Use for anything over time (rates, latencies, counts per bucket).

```json
{
  "id": {"value": "<uuid>"},
  "title": "Latency P95 by Stage",
  "definition": {
    "lineChart": {
      "legend": {"isVisible": true, "columns": [], "groupByQuery": true, "placement": "LEGEND_PLACEMENT_AUTO"},
      "tooltip": {"showLabels": false, "type": "TOOLTIP_TYPE_ALL"},
      "queryDefinitions": [
        {
          "id": "<uuid>",
          "query": {
            "metrics": {
              "promqlQuery": {"value": "histogram_quantile(0.95, sum by (le, stage) (rate(foo_latency_bucket[${__range}])))"},
              "filters": [],
              "editorMode": "METRICS_QUERY_EDITOR_MODE_TEXT",
              "seriesLimitType": "METRICS_SERIES_LIMIT_TYPE_BY_SERIES_COUNT"
            }
          },
          "seriesCountLimit": "20",
          "unit": "UNIT_SECONDS",
          "scaleType": "SCALE_TYPE_LINEAR",
          "isVisible": true,
          "colorScheme": "classic",
          "resolution": {"bucketsPresented": 96},
          "dataModeType": "DATA_MODE_TYPE_HIGH_UNSPECIFIED",
          "decimal": 2,
          "hashColors": false,
          "decimalPrecision": false,
          "intervalResolution": {"auto": {"minimumInterval": "15s", "maximumDataPoints": 96}}
        }
      ],
      "stackedLine": "STACKED_LINE_UNSPECIFIED",
      "connectNulls": false
    }
  }
}
```

- For counts (not latency) use `"unit": "UNIT_UNSPECIFIED"`.
- Multiple lines in the same panel: add more objects to `queryDefinitions` (each with its own `id`).

---

## Widget: dataTable

Use for top-N tables and raw log listings.

**Metrics-backed table** (e.g. top accounts by count):

```json
{
  "id": {"value": "<uuid>"},
  "title": "Top Accounts by Message Count",
  "definition": {
    "dataTable": {
      "query": {
        "metrics": {
          "promqlQuery": {"value": "sum by (account_id) (increase(foo_total[${__range}]))"},
          "filters": [],
          "editorMode": "METRICS_QUERY_EDITOR_MODE_TEXT",
          "promqlQueryType": "PROM_QL_QUERY_TYPE_INSTANT"
        }
      },
      "resultsPerPage": 10,
      "rowStyle": "ROW_STYLE_UNSPECIFIED",
      "columns": [
        {"field": "account_id"},
        {"field": "#value"}
      ],
      "dataModeType": "DATA_MODE_TYPE_HIGH_UNSPECIFIED"
    }
  }
}
```

**DataPrime-backed table** (e.g. last errors):

```json
{
  "id": {"value": "<uuid>"},
  "title": "Last errors",
  "definition": {
    "dataTable": {
      "query": {
        "dataprime": {
          "dataprimeQuery": {"text": "source logs | filter $m.severity == ERROR || $m.severity == CRITICAL | orderby $m.timestamp desc"},
          "filters": []
        }
      },
      "resultsPerPage": 100,
      "rowStyle": "ROW_STYLE_ONE_LINE",
      "columns": [
        {"field": "$m.severity",  "width": 121},
        {"field": "$m.timestamp", "width": 190},
        {"field": "$d",           "width": 600}
      ],
      "dataModeType": "DATA_MODE_TYPE_HIGH_UNSPECIFIED"
    }
  }
}
```

---

## Top-level filters

Add one entry per slicing dimension the user approved. Users fill in `values` at view time.

**Exclude non-prod environments** (replace the example values with whatever the target deployment actually uses, e.g. `["dev", "staging", "test"]`):

```json
{
  "source": {
    "metrics": {
      "label": "subsystem_name",
      "operator": {"notEquals": {"selection": {"list": {"values": ["<non-prod-env-1>", "<non-prod-env-2>"]}}}}
    }
  },
  "enabled": true,
  "collapsed": false,
  "id": {"value": "<uuid>"}
}
```

**User-fillable slicing filter**:

```json
{
  "source": {
    "metrics": {
      "label": "account_id",
      "operator": {"equals": {"selection": {"list": {"values": []}}}}
    }
  },
  "enabled": true,
  "collapsed": false,
  "id": {"value": "<uuid>"}
}
```

---

## Section template

```json
{
  "id": {"value": "<uuid>"},
  "rows": [ /* rows go here */ ],
  "options": {
    "custom": {
      "name": "Errors",
      "collapsed": true,
      "color": {"predefined": "SECTION_PREDEFINED_COLOR_UNSPECIFIED"}
    }
  }
}
```

Set `collapsed: true` for logs/debug sections and any section that isn't the dashboard's primary purpose.

---

## Tier (`dataModeType`) quick-reference

`dataModeType` lives **per widget** under the widget's definition (e.g. `definition.gauge.dataModeType`, `definition.dataTable.dataModeType`, `definition.lineChart.queryDefinitions[].dataModeType`). It controls which storage tier the widget reads from at render time.

| Value | When |
|---|---|
| `DATA_MODE_TYPE_HIGH_UNSPECIFIED` | default — frequent (hot) search tier |
| `DATA_MODE_TYPE_ARCHIVE` | archive (cold) tier — for long lookbacks or when the user requests archive |

The templates above ship with `DATA_MODE_TYPE_HIGH_UNSPECIFIED`. When the user wants archive, replace `DATA_MODE_TYPE_HIGH_UNSPECIFIED` → `DATA_MODE_TYPE_ARCHIVE` on **every** widget before deploy (not just the dashboard root).

---

## Unit enum quick-reference

| Value | When |
|---|---|
| `UNIT_UNSPECIFIED` | raw counts, unitless ratios |
| `UNIT_NUMBER` | explicit integer count |
| `UNIT_SECONDS` | durations/latency |
| `UNIT_PERCENT` | percentages 0–100 |
| `UNIT_BYTES` | sizes |

---

## Threshold type

- `THRESHOLD_TYPE_ABSOLUTE` - thresholds compared against the raw value. Use for success rates and fixed-meaning counts.
- `THRESHOLD_TYPE_RELATIVE` - thresholds as % of min/max. Use when the scale is arbitrary.

Default to `ABSOLUTE` for rates and DLQ counts; `RELATIVE` only when the absolute scale is unknown.
