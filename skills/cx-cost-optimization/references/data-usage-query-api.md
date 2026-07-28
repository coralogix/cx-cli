# Data Usage Query API Reference

Use this reference only after running `cx usage capabilities -o json` in the current session. All labels, measurement kinds, known values, and limits are tenant-specific.

## Capabilities Response

```json
{
  "supportedLabels": [
    {
      "key": "string",
      "keyDescription": "string",
      "highCardinality": true,
      "knownValues": [
        {
          "value": "string",
          "description": "string"
        }
      ]
    }
  ],
  "supportedMeasurements": [
    {
      "kind": "MEASUREMENT_KIND_*",
      "description": "string",
      "unit": "MEASUREMENT_UNIT_*"
    }
  ],
  "maxUsageEntriesPerMessage": "integer",
  "maxGroupByLabels": "integer",
  "maxFilters": "integer",
  "maxFilterValues": "integer"
}
```

Use `supportedLabels[].key` in `groupBy.keys` or `filters[].key`. Use a label's `knownValues[].value` only with that same filter key. Do not send descriptions or units in the query.

## Query Request

```json
{
  "daily": {
    "relativeRange": "DAILY_RELATIVE_RANGE_LAST_7_DAYS",
    "dateRange": {
      "start": {"year": 2026, "month": 7, "day": 1},
      "end": {"year": 2026, "month": 7, "day": 8}
    }
  },
  "hourly": {
    "relativeRange": "HOURLY_RELATIVE_RANGE_LAST_24_HOURS",
    "timeRange": {
      "start": "2026-07-20T00:00:00Z",
      "end": "2026-07-21T00:00:00Z"
    }
  },
  "groupBy": {
    "keys": ["capabilities.supportedLabels[].key"]
  },
  "filters": [
    {
      "key": "capabilities.supportedLabels[].key",
      "operator": "FILTER_OPERATOR_IN | FILTER_OPERATOR_NOT_IN",
      "values": ["capabilities.supportedLabels[].knownValues[].value"]
    }
  ],
  "measurementKindFilter": [
    "capabilities.supportedMeasurements[].kind"
  ],
  "limit": {
    "perBucket": 10
  }
}
```

The example shows all optional fields together for reference; do not send both `daily` and `hourly`, or both range forms within one interval.

## Interval and Limit Rules

- Send exactly one of `daily` or `hourly`.
- For `daily`, send exactly one of `relativeRange` or `dateRange`. Absolute dates are UTC and half-open: `[start, end)`.
- For `hourly`, send exactly one of `relativeRange` or `timeRange`. Absolute timestamps must be UTC, aligned to the hour, and half-open: `[start, end)`.
- `groupBy`, `filters`, `measurementKindFilter`, and `limit` are optional.
- Do not exceed `maxGroupByLabels`, `maxFilters`, or `maxFilterValues` from capabilities.
- Keep `limit.perBucket` and grouping scope small enough to stay within `maxUsageEntriesPerMessage`.
- Filter a high-cardinality label before grouping by it.

## Query Response

```json
{
  "queryRange": {
    "start": "RFC3339 UTC timestamp",
    "end": "RFC3339 UTC timestamp"
  },
  "buckets": [
    {
      "range": {
        "start": "RFC3339 UTC timestamp",
        "end": "RFC3339 UTC timestamp"
      },
      "entries": [
        {
          "labels": [
            {
              "key": "string",
              "value": "string"
            }
          ],
          "measurements": [
            {
              "kind": "a supported measurement kind",
              "measuredValue": "uint64 encoded as string",
              "measuredUnit": "the unit for that kind",
              "cxQuotaUnits": {
                "value": "decimal string"
              }
            }
          ]
        }
      ]
    }
  ]
}
```

`queryRange` is the server-resolved interval. Each bucket is one UTC day or hour. Each entry is one unique group-by label combination. `measuredValue` is raw usage; `cxQuotaUnits.value` is the billable amount.
