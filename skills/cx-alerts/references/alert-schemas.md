# Alert Definition Schemas Reference

Complete JSON schema reference for all Coralogix alert types, using the **actual REST API wire format**. Use this when constructing `alertDefProperties` payloads for `cx alerts create`.

> **Tip:** The easiest way to create a new alert is to fetch an existing one with `cx alerts get <id> -o json`, modify the JSON, and pipe it into `cx alerts create --from-file -`.

## Common Structure

Every alert definition has this top-level shape. The alert type config (e.g. `logsThreshold`) is a **sibling** of `type`, `name`, `priority`, etc. - NOT nested inside `type`.

```json
{
  "alertDefProperties": {
    "name": "My Alert (required)",
    "description": "What this alert monitors",
    "priority": "ALERT_DEF_PRIORITY_P3",
    "type": "ALERT_DEF_TYPE_LOGS_THRESHOLD",
    "enabled": true,
    "groupByKeys": [],
    "entityLabels": {},
    "phantomMode": false,
    "activeOn": { ... },
    "incidentsSettings": { ... },
    "notificationGroup": { ... },
    "logsThreshold": { ... }
  }
}
```

### Priority values

| Wire value | Meaning |
|---|---|
| `ALERT_DEF_PRIORITY_P1` | Critical |
| `ALERT_DEF_PRIORITY_P2` | High |
| `ALERT_DEF_PRIORITY_P3` | Medium |
| `ALERT_DEF_PRIORITY_P4` | Low |
| `ALERT_DEF_PRIORITY_P5_OR_UNSPECIFIED` | Info |

### Type values

| Wire value | Alert type config key |
|---|---|
| `ALERT_DEF_TYPE_LOGS_IMMEDIATE_OR_UNSPECIFIED` | `logsImmediate` |
| `ALERT_DEF_TYPE_LOGS_THRESHOLD` | `logsThreshold` |
| `ALERT_DEF_TYPE_LOGS_ANOMALY` | `logsAnomaly` |
| `ALERT_DEF_TYPE_LOGS_RATIO_THRESHOLD` | `logsRatioThreshold` |
| `ALERT_DEF_TYPE_LOGS_NEW_VALUE` | `logsNewValue` |
| `ALERT_DEF_TYPE_LOGS_UNIQUE_COUNT` | `logsUniqueCount` |
| `ALERT_DEF_TYPE_LOGS_TIME_RELATIVE_THRESHOLD` | `logsTimeRelativeThreshold` |
| `ALERT_DEF_TYPE_METRIC_THRESHOLD` | `metricThreshold` |
| `ALERT_DEF_TYPE_METRIC_ANOMALY` | `metricAnomaly` |
| `ALERT_DEF_TYPE_TRACING_IMMEDIATE` | `tracingImmediate` |
| `ALERT_DEF_TYPE_TRACING_THRESHOLD` | `tracingThreshold` |
| `ALERT_DEF_TYPE_FLOW` | `flow` |

---

## Common Sub-Objects

### Activity Schedule (`activeOn`)

```json
{
  "dayOfWeek": ["DAY_OF_WEEK_MONDAY_OR_UNSPECIFIED", "DAY_OF_WEEK_TUESDAY"],
  "startTime": { "hours": 8, "minutes": 0 },
  "endTime": { "hours": 18, "minutes": 0 }
}
```

Day values: `DAY_OF_WEEK_MONDAY_OR_UNSPECIFIED`, `DAY_OF_WEEK_TUESDAY`, `DAY_OF_WEEK_WEDNESDAY`, `DAY_OF_WEEK_THURSDAY`, `DAY_OF_WEEK_FRIDAY`, `DAY_OF_WEEK_SATURDAY`, `DAY_OF_WEEK_SUNDAY`

### Incident Settings (`incidentsSettings`)

```json
{
  "minutes": 60,
  "notifyOn": "NOTIFY_ON_TRIGGERED_ONLY_UNSPECIFIED"
}
```

`notifyOn` values: `NOTIFY_ON_TRIGGERED_ONLY_UNSPECIFIED`, `NOTIFY_ON_TRIGGERED_AND_RESOLVED`

### Notification Group (`notificationGroup`)

```json
{
  "groupByKeys": [],
  "destinations": [
    {
      "connectorId": "uuid",
      "presetId": "uuid",
      "notifyOn": "NOTIFY_ON_TRIGGERED_AND_RESOLVED"
    }
  ],
  "webhooks": [
    {
      "integration": { "integrationId": 123 },
      "minutes": 15,
      "notifyOn": "NOTIFY_ON_TRIGGERED_ONLY_UNSPECIFIED"
    }
  ],
  "router": {
    "id": "uuid",
    "notifyOn": "NOTIFY_ON_TRIGGERED_ONLY_UNSPECIFIED"
  }
}
```

### Logs Filter (`logsFilter`)

Used by all log-based alert types:

```json
{
  "simpleFilter": {
    "luceneQuery": "severity:ERROR AND service:api",
    "labelFilters": {
      "applicationName": [
        { "operation": "LOG_FILTER_OPERATION_TYPE_IS_OR_UNSPECIFIED", "value": "my-app" }
      ],
      "subsystemName": [
        { "operation": "LOG_FILTER_OPERATION_TYPE_STARTS_WITH", "value": "backend" }
      ],
      "severities": ["LOG_SEVERITY_WARNING", "LOG_SEVERITY_ERROR", "LOG_SEVERITY_CRITICAL"]
    }
  }
}
```

**Label filter operations:** `LOG_FILTER_OPERATION_TYPE_IS_OR_UNSPECIFIED`, `LOG_FILTER_OPERATION_TYPE_INCLUDES`, `LOG_FILTER_OPERATION_TYPE_STARTS_WITH`, `LOG_FILTER_OPERATION_TYPE_ENDS_WITH`

**Severities:** `LOG_SEVERITY_VERBOSE_UNSPECIFIED`, `LOG_SEVERITY_DEBUG`, `LOG_SEVERITY_INFO`, `LOG_SEVERITY_WARNING`, `LOG_SEVERITY_ERROR`, `LOG_SEVERITY_CRITICAL`

### Tracing Filter (`tracingFilter`)

Used by tracing-based alert types:

```json
{
  "simpleFilter": {
    "latencyThresholdMs": "1000",
    "tracingLabelFilters": {
      "applicationName": [
        { "operation": "TRACING_FILTER_OPERATION_TYPE_IS_OR_UNSPECIFIED", "values": ["my-app"] }
      ],
      "serviceName": [
        { "operation": "TRACING_FILTER_OPERATION_TYPE_IS_OR_UNSPECIFIED", "values": ["api-gateway"] }
      ],
      "operationName": [],
      "subsystemName": [],
      "spanFields": [
        {
          "key": "http.status_code",
          "filterType": {
            "operation": "TRACING_FILTER_OPERATION_TYPE_IS_OR_UNSPECIFIED",
            "values": ["500"]
          }
        }
      ]
    }
  }
}
```

**Tracing filter operations:** `TRACING_FILTER_OPERATION_TYPE_IS_OR_UNSPECIFIED`, `TRACING_FILTER_OPERATION_TYPE_INCLUDES`, `TRACING_FILTER_OPERATION_TYPE_STARTS_WITH`, `TRACING_FILTER_OPERATION_TYPE_ENDS_WITH`, `TRACING_FILTER_OPERATION_TYPE_IS_NOT`

### Undetected Values Management

```json
{
  "triggerUndetectedValues": true,
  "autoRetireTimeframe": "AUTO_RETIRE_TIMEFRAME_HOUR_1"
}
```

Values: `AUTO_RETIRE_TIMEFRAME_NEVER_OR_UNSPECIFIED`, `AUTO_RETIRE_TIMEFRAME_MINUTES_5`, `AUTO_RETIRE_TIMEFRAME_MINUTES_10`, `AUTO_RETIRE_TIMEFRAME_HOUR_1`, `AUTO_RETIRE_TIMEFRAME_HOURS_2`, `AUTO_RETIRE_TIMEFRAME_HOURS_6`, `AUTO_RETIRE_TIMEFRAME_HOURS_12`, `AUTO_RETIRE_TIMEFRAME_HOURS_24`

---

## Alert Type Schemas

Each section shows only the alert-type-specific config block. This block is placed as a sibling of `type`, `name`, `priority`, etc. inside `alertDefProperties`.

### 1. Logs Threshold (`logsThreshold`)

Trigger when log count crosses a threshold in a time window.

```json
{
  "alertDefProperties": {
    "name": "High Error Rate",
    "description": "Alert when error logs exceed threshold",
    "priority": "ALERT_DEF_PRIORITY_P2",
    "type": "ALERT_DEF_TYPE_LOGS_THRESHOLD",
    "enabled": true,
    "logsThreshold": {
      "logsFilter": {
        "simpleFilter": {
          "luceneQuery": "severity:ERROR",
          "labelFilters": {
            "applicationName": [
              { "operation": "LOG_FILTER_OPERATION_TYPE_IS_OR_UNSPECIFIED", "value": "my-app" }
            ]
          }
        }
      },
      "rules": [{
        "condition": {
          "conditionType": "LOGS_THRESHOLD_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED",
          "threshold": 100,
          "timeWindow": {
            "logsTimeWindowSpecificValue": "LOGS_TIME_WINDOW_VALUE_MINUTES_5_OR_UNSPECIFIED"
          }
        },
        "override": { "priority": "ALERT_DEF_PRIORITY_P1" }
      }],
      "notificationPayloadFilter": [],
      "undetectedValuesManagement": null,
      "evaluationDelayMs": 0
    }
  }
}
```

**conditionType:** `LOGS_THRESHOLD_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED`, `LOGS_THRESHOLD_CONDITION_TYPE_LESS_THAN`

**logsTimeWindowSpecificValue:** `LOGS_TIME_WINDOW_VALUE_MINUTES_5_OR_UNSPECIFIED`, `LOGS_TIME_WINDOW_VALUE_MINUTES_10`, `LOGS_TIME_WINDOW_VALUE_MINUTES_15`, `LOGS_TIME_WINDOW_VALUE_MINUTES_20`, `LOGS_TIME_WINDOW_VALUE_MINUTES_30`, `LOGS_TIME_WINDOW_VALUE_HOUR_1`, `LOGS_TIME_WINDOW_VALUE_HOURS_2`, `LOGS_TIME_WINDOW_VALUE_HOURS_4`, `LOGS_TIME_WINDOW_VALUE_HOURS_6`, `LOGS_TIME_WINDOW_VALUE_HOURS_12`, `LOGS_TIME_WINDOW_VALUE_HOURS_24`, `LOGS_TIME_WINDOW_VALUE_HOURS_36`

### 2. Logs Immediate (`logsImmediate`)

Trigger instantly on every matching log entry. No rules or time windows.

```json
{
  "alertDefProperties": {
    "name": "OOM Killer Detected",
    "description": "Alert immediately when OOM killer runs",
    "priority": "ALERT_DEF_PRIORITY_P1",
    "type": "ALERT_DEF_TYPE_LOGS_IMMEDIATE_OR_UNSPECIFIED",
    "enabled": true,
    "logsImmediate": {
      "logsFilter": {
        "simpleFilter": {
          "luceneQuery": "\"Out of memory\" OR \"OOM\"",
          "labelFilters": {}
        }
      },
      "notificationPayloadFilter": []
    }
  }
}
```

### 3. Logs Anomaly (`logsAnomaly`)

ML-based anomaly detection on log volume.

```json
{
  "alertDefProperties": {
    "name": "Unusual Log Volume",
    "priority": "ALERT_DEF_PRIORITY_P3",
    "type": "ALERT_DEF_TYPE_LOGS_ANOMALY",
    "enabled": true,
    "logsAnomaly": {
      "logsFilter": { "simpleFilter": { "luceneQuery": "*", "labelFilters": {} } },
      "rules": [{
        "condition": {
          "conditionType": "LOGS_ANOMALY_CONDITION_TYPE_MORE_THAN_USUAL_OR_UNSPECIFIED",
          "minimumThreshold": 10,
          "timeWindow": {
            "logsTimeWindowSpecificValue": "LOGS_TIME_WINDOW_VALUE_HOUR_1"
          }
        }
      }],
      "anomalyAlertSettings": { "percentageOfDeviation": 50 },
      "notificationPayloadFilter": [],
      "evaluationDelayMs": 0
    }
  }
}
```

**conditionType:** `LOGS_ANOMALY_CONDITION_TYPE_MORE_THAN_USUAL_OR_UNSPECIFIED`

### 4. Logs Ratio Threshold (`logsRatioThreshold`)

Alert based on ratio between two log queries.

```json
{
  "alertDefProperties": {
    "name": "Error Rate Ratio",
    "priority": "ALERT_DEF_PRIORITY_P2",
    "type": "ALERT_DEF_TYPE_LOGS_RATIO_THRESHOLD",
    "enabled": true,
    "logsRatioThreshold": {
      "numerator": { "simpleFilter": { "luceneQuery": "severity:ERROR", "labelFilters": {} } },
      "numeratorAlias": "Errors",
      "denominator": { "simpleFilter": { "luceneQuery": "*", "labelFilters": {} } },
      "denominatorAlias": "All Logs",
      "rules": [{
        "condition": {
          "conditionType": "LOGS_RATIO_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED",
          "threshold": 0.1,
          "timeWindow": {
            "logsRatioTimeWindowSpecificValue": "LOGS_RATIO_TIME_WINDOW_VALUE_MINUTES_5_OR_UNSPECIFIED"
          }
        }
      }],
      "groupByFor": "LOGS_RATIO_GROUP_BY_FOR_BOTH_OR_UNSPECIFIED",
      "ignoreInfinity": true,
      "notificationPayloadFilter": []
    }
  }
}
```

**conditionType:** `LOGS_RATIO_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED`, `LOGS_RATIO_CONDITION_TYPE_LESS_THAN`

**groupByFor:** `LOGS_RATIO_GROUP_BY_FOR_BOTH_OR_UNSPECIFIED`, `LOGS_RATIO_GROUP_BY_FOR_NUMERATOR_ONLY`, `LOGS_RATIO_GROUP_BY_FOR_DENUMERATOR_ONLY`

**logsRatioTimeWindowSpecificValue:** `LOGS_RATIO_TIME_WINDOW_VALUE_MINUTES_5_OR_UNSPECIFIED`, `..._MINUTES_10`, `..._MINUTES_15`, `..._MINUTES_30`, `..._HOUR_1`, `..._HOURS_2`, `..._HOURS_4`, `..._HOURS_6`, `..._HOURS_12`, `..._HOURS_24`, `..._HOURS_36`

### 5. Logs Time Relative Threshold (`logsTimeRelativeThreshold`)

Compare current log volume to a past time period.

```json
{
  "alertDefProperties": {
    "name": "Spike vs Yesterday",
    "priority": "ALERT_DEF_PRIORITY_P3",
    "type": "ALERT_DEF_TYPE_LOGS_TIME_RELATIVE_THRESHOLD",
    "enabled": true,
    "logsTimeRelativeThreshold": {
      "logsFilter": { "simpleFilter": { "luceneQuery": "severity:ERROR", "labelFilters": {} } },
      "rules": [{
        "condition": {
          "conditionType": "LOGS_TIME_RELATIVE_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED",
          "threshold": 1.5,
          "comparedTo": "LOGS_TIME_RELATIVE_COMPARED_TO_SAME_HOUR_YESTERDAY"
        }
      }],
      "ignoreInfinity": true,
      "notificationPayloadFilter": []
    }
  }
}
```

**conditionType:** `LOGS_TIME_RELATIVE_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED`, `LOGS_TIME_RELATIVE_CONDITION_TYPE_LESS_THAN`

**comparedTo:** `LOGS_TIME_RELATIVE_COMPARED_TO_PREVIOUS_HOUR_OR_UNSPECIFIED`, `..._SAME_HOUR_YESTERDAY`, `..._SAME_HOUR_LAST_WEEK`, `..._YESTERDAY`, `..._SAME_DAY_LAST_WEEK`, `..._SAME_DAY_LAST_MONTH`

### 6. Logs Unique Count (`logsUniqueCount`)

Alert when unique value count in a field crosses a threshold.

```json
{
  "alertDefProperties": {
    "name": "Too Many Unique IPs",
    "priority": "ALERT_DEF_PRIORITY_P3",
    "type": "ALERT_DEF_TYPE_LOGS_UNIQUE_COUNT",
    "enabled": true,
    "logsUniqueCount": {
      "logsFilter": { "simpleFilter": { "luceneQuery": "*", "labelFilters": {} } },
      "uniqueCountKeypath": "remote_addr",
      "maxUniqueCountPerGroupByKey": "1000",
      "rules": [{
        "condition": {
          "maxUniqueCount": "500",
          "timeWindow": {
            "logsUniqueValueTimeWindowSpecificValue": "LOGS_UNIQUE_VALUE_TIME_WINDOW_VALUE_MINUTES_5"
          }
        }
      }],
      "notificationPayloadFilter": []
    }
  }
}
```

**logsUniqueValueTimeWindowSpecificValue:** `LOGS_UNIQUE_VALUE_TIME_WINDOW_VALUE_MINUTE_1_OR_UNSPECIFIED`, `..._MINUTES_5`, `..._MINUTES_10`, `..._MINUTES_15`, `..._MINUTES_20`, `..._MINUTES_30`, `..._HOURS_1`, `..._HOURS_2`, `..._HOURS_4`, `..._HOURS_6`, `..._HOURS_12`, `..._HOURS_24`, `..._HOURS_36`

### 7. Logs New Value (`logsNewValue`)

Alert when a value not previously seen appears in a field.

```json
{
  "alertDefProperties": {
    "name": "New IP Address",
    "priority": "ALERT_DEF_PRIORITY_P3",
    "type": "ALERT_DEF_TYPE_LOGS_NEW_VALUE",
    "enabled": true,
    "logsNewValue": {
      "logsFilter": { "simpleFilter": { "luceneQuery": "*", "labelFilters": {} } },
      "rules": [{
        "condition": {
          "keypathToTrack": "ip_address",
          "timeWindow": {
            "logsNewValueTimeWindowSpecificValue": "LOGS_NEW_VALUE_TIME_WINDOW_VALUE_HOURS_24"
          }
        }
      }],
      "notificationPayloadFilter": []
    }
  }
}
```

**logsNewValueTimeWindowSpecificValue:** `LOGS_NEW_VALUE_TIME_WINDOW_VALUE_HOURS_12_OR_UNSPECIFIED`, `..._HOURS_24`, `..._HOURS_48`, `..._HOURS_72`, `..._WEEK_1`, `..._MONTH_1`, `..._MONTHS_2`, `..._MONTHS_3`

### 8. Metric Threshold (`metricThreshold`)

Trigger when a PromQL expression crosses a threshold.

```json
{
  "alertDefProperties": {
    "name": "CPU Usage Critical",
    "priority": "ALERT_DEF_PRIORITY_P1",
    "type": "ALERT_DEF_TYPE_METRIC_THRESHOLD",
    "enabled": true,
    "metricThreshold": {
      "metricFilter": {
        "promql": "avg(cpu_usage_percent{service=\"api\"})"
      },
      "rules": [{
        "condition": {
          "conditionType": "METRIC_THRESHOLD_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED",
          "threshold": 90,
          "ofTheLast": {
            "dynamicDuration": "5m"
          },
          "forOverPct": 100
        }
      }],
      "missingValues": {
        "replaceWithZero": true,
        "minNonNullValuesPct": 0
      },
      "undetectedValuesManagement": null,
      "evaluationDelayMs": 0
    }
  }
}
```

**conditionType:** `METRIC_THRESHOLD_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED`, `..._MORE_THAN_OR_EQUALS`, `..._LESS_THAN`, `..._LESS_THAN_OR_EQUALS`

**dynamicDuration:** any PromQL duration string (e.g. `5m`, `1h`, `24h`) within 1-2160 minutes. This is a free-form string, not an enum.

**forOverPct:** percentage of data points that must breach (0-100).

### 9. Metric Anomaly (`metricAnomaly`)

ML-based anomaly detection on metrics.

```json
{
  "alertDefProperties": {
    "name": "Unusual Request Rate",
    "priority": "ALERT_DEF_PRIORITY_P3",
    "type": "ALERT_DEF_TYPE_METRIC_ANOMALY",
    "enabled": true,
    "metricAnomaly": {
      "metricFilter": {
        "promql": "rate(http_requests_total[5m])"
      },
      "rules": [{
        "condition": {
          "conditionType": "METRIC_ANOMALY_CONDITION_TYPE_MORE_THAN_USUAL_OR_UNSPECIFIED",
          "threshold": 10,
          "ofTheLast": {
            "specificValue": "METRIC_TIME_WINDOW_VALUE_HOUR_1"
          },
          "forOverPct": 100,
          "minNonNullValuesPct": 50
        }
      }],
      "anomalyAlertSettings": { "percentageOfDeviation": 50 },
      "evaluationDelayMs": 0
    }
  }
}
```

**conditionType:** `METRIC_ANOMALY_CONDITION_TYPE_MORE_THAN_USUAL_OR_UNSPECIFIED`, `METRIC_ANOMALY_CONDITION_TYPE_LESS_THAN_USUAL`

**specificValue (timeWindow):** `METRIC_TIME_WINDOW_VALUE_MINUTES_1_OR_UNSPECIFIED`, `..._MINUTES_5`, `..._MINUTES_10`, `..._MINUTES_15`, `..._MINUTES_20`, `..._MINUTES_30`, `..._HOUR_1`, `..._HOURS_2`, `..._HOURS_4`, `..._HOURS_6`, `..._HOURS_12`, `..._HOURS_24`, `..._HOURS_36`

### 10. Tracing Immediate (`tracingImmediate`)

Trigger instantly on matching trace spans.

```json
{
  "alertDefProperties": {
    "name": "Slow API Call",
    "priority": "ALERT_DEF_PRIORITY_P2",
    "type": "ALERT_DEF_TYPE_TRACING_IMMEDIATE",
    "enabled": true,
    "tracingImmediate": {
      "tracingFilter": {
        "simpleFilter": {
          "latencyThresholdMs": "1000",
          "tracingLabelFilters": {
            "serviceName": [
              {
                "operation": "TRACING_FILTER_OPERATION_TYPE_IS_OR_UNSPECIFIED",
                "values": ["api-gateway"]
              }
            ],
            "applicationName": [],
            "operationName": [],
            "subsystemName": [],
            "spanFields": []
          }
        }
      },
      "notificationPayloadFilter": []
    }
  }
}
```

### 11. Tracing Threshold (`tracingThreshold`)

Trigger when span count crosses a threshold.

```json
{
  "alertDefProperties": {
    "name": "High Span Volume",
    "priority": "ALERT_DEF_PRIORITY_P3",
    "type": "ALERT_DEF_TYPE_TRACING_THRESHOLD",
    "enabled": true,
    "tracingThreshold": {
      "tracingFilter": {
        "simpleFilter": {
          "latencyThresholdMs": "0",
          "tracingLabelFilters": {
            "serviceName": [
              {
                "operation": "TRACING_FILTER_OPERATION_TYPE_IS_OR_UNSPECIFIED",
                "values": ["api-gateway"]
              }
            ],
            "applicationName": [],
            "operationName": [],
            "subsystemName": [],
            "spanFields": []
          }
        }
      },
      "rules": [{
        "condition": {
          "conditionType": "TRACING_THRESHOLD_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED",
          "spanAmount": 100,
          "timeWindow": {
            "tracingTimeWindowSpecificValue": "TRACING_TIME_WINDOW_VALUE_MINUTES_5_OR_UNSPECIFIED"
          }
        }
      }],
      "notificationPayloadFilter": []
    }
  }
}
```

**conditionType:** `TRACING_THRESHOLD_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED`

**tracingTimeWindowSpecificValue:** `TRACING_TIME_WINDOW_VALUE_MINUTES_5_OR_UNSPECIFIED`, `..._MINUTES_10`, `..._MINUTES_15`, `..._MINUTES_20`, `..._MINUTES_30`, `..._HOUR_1`, `..._HOURS_2`, `..._HOURS_4`, `..._HOURS_6`, `..._HOURS_12`, `..._HOURS_24`, `..._HOURS_36`

### 12. SLO Threshold (`sloThreshold`)

Monitor error budget consumption or burn rate. Exactly one of `errorBudget` or `burnRate` must be set.

#### Error Budget variant:

```json
{
  "alertDefProperties": {
    "name": "SLO Budget Low",
    "priority": "ALERT_DEF_PRIORITY_P2",
    "type": "ALERT_DEF_TYPE_SLO_THRESHOLD",
    "enabled": true,
    "sloThreshold": {
      "sloDefinition": { "sloId": "uuid" },
      "errorBudget": {
        "rules": [{
          "condition": { "threshold": 50 },
          "override": { "priority": "ALERT_DEF_PRIORITY_P1" }
        }]
      }
    }
  }
}
```

#### Burn Rate variant:

```json
{
  "alertDefProperties": {
    "name": "SLO Burn Rate High",
    "priority": "ALERT_DEF_PRIORITY_P1",
    "type": "ALERT_DEF_TYPE_SLO_THRESHOLD",
    "enabled": true,
    "sloThreshold": {
      "sloDefinition": { "sloId": "uuid" },
      "burnRate": {
        "rules": [{
          "condition": { "threshold": 2.0 },
          "override": { "priority": "ALERT_DEF_PRIORITY_P1" }
        }],
        "single": {
          "timeDuration": { "duration": "1", "unit": "DURATION_UNIT_HOURS" }
        }
      }
    }
  }
}
```

`unit` values: `DURATION_UNIT_UNSPECIFIED`, `DURATION_UNIT_HOURS`

---

## Important Notes

- **groupByKeys for metric alerts**: Leave empty to let the API infer from the PromQL `by` clause. If provided, keys must be in **alphabetical order** (the API infers alphabetically, not in query order).
- **Priority**: Always ask the user -- never pick a default.
- **override in rules**: Optional per-rule priority override using the same `ALERT_DEF_PRIORITY_*` enum values. Omit to use the alert-level priority.
- **notificationPayloadFilter**: List of log/span field paths to include in notifications (e.g. `["obj.field"]`).
- **Best practice**: Fetch an existing alert with `cx alerts get <id> -o json` to see the exact response shape, then use it as a template for creating new alerts.
