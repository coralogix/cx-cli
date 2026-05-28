---
name: cx-alerts
description: This skill should be used when the user asks to "manage alerts", "create alert", "list alerts", "delete alert", "check alert status", "enable alert", "disable alert", "investigate firing alerts", "check which alerts are active", "find alerting rules", "set up an alert", "configure alerting", "mute an alert", "silence an alert", "see alert definitions", "check alert priority", or wants to manage Coralogix alert definitions using the cx CLI.
metadata:
  version: "0.1.0"
---

# Alert Management Skill

Use this skill to list, inspect, create, delete, enable, and disable Coralogix alert definitions using the `cx alerts` CLI commands.

## CLI Commands

| Command | Purpose | Key flags |
|---|---|---|
| `cx alerts list` | List all alert definitions | `--name <filter>` |
| `cx alerts get <id>` | Get a single alert definition by ID | - |
| `cx alerts create` | Create an alert from a JSON definition | `--from-file <path>` (default: stdin) |
| `cx alerts delete <id>` | Delete an alert | - |
| `cx alerts enable <id>` | Enable an alert | - |
| `cx alerts disable <id>` | Disable an alert | - |
| `cx alerts events` | List events; use alert-version scoped endpoint when filtering | `--alert-version-id`, `--start`, `--end` |
| `cx alerts event-stats` | Get alert event statistics | - |
| `cx alerts suppression-rules list` | List suppression rules | - |
| `cx alerts suppression-rules get <id>` | Get a suppression rule | - |
| `cx alerts suppression-rules create` | Create a suppression rule | `--from-file <path>` |
| `cx alerts suppression-rules update` | Update a suppression rule | `--from-file <path>` |
| `cx alerts suppression-rules delete <id>` | Delete a suppression rule | - |

**Output format:** append `-o json` or `-o agents` to `list`, `get`, and `create` commands for machine-readable output.

**Multi-profile:** use `-p <profile>` (repeatable) to target multiple profiles simultaneously.

## Alert Types Reference

Coralogix supports 12 alert types:

| Type enum | Human name | Description |
|---|---|---|
| `ALERT_DEF_TYPE_LOGS_IMMEDIATE` | Logs Immediate | Trigger on every matching log entry |
| `ALERT_DEF_TYPE_LOGS_THRESHOLD` | Logs Threshold | Trigger when log count exceeds a threshold in a time window |
| `ALERT_DEF_TYPE_LOGS_ANOMALY` | Logs Anomaly | ML-based anomaly detection on log volume |
| `ALERT_DEF_TYPE_LOGS_RATIO_THRESHOLD` | Logs Ratio Threshold | Trigger on ratio between two log queries |
| `ALERT_DEF_TYPE_LOGS_NEW_VALUE` | Logs New Value | Trigger when a new value appears in a field |
| `ALERT_DEF_TYPE_LOGS_UNIQUE_COUNT` | Logs Unique Count | Trigger on unique value count threshold |
| `ALERT_DEF_TYPE_LOGS_TIME_RELATIVE_THRESHOLD` | Logs Time Relative | Compare current vs past time window |
| `ALERT_DEF_TYPE_METRIC_THRESHOLD` | Metric Threshold | Trigger when a PromQL expression crosses a threshold |
| `ALERT_DEF_TYPE_METRIC_ANOMALY` | Metric Anomaly | ML-based anomaly detection on metrics |
| `ALERT_DEF_TYPE_TRACING_IMMEDIATE` | Tracing Immediate | Trigger on every matching span |
| `ALERT_DEF_TYPE_TRACING_THRESHOLD` | Tracing Threshold | Trigger when span count exceeds a threshold |
| `ALERT_DEF_TYPE_FLOW` | Flow | Sequence-based alert combining multiple conditions |

## Priority Levels

Always ask the user what priority to use when creating alerts:

| Priority | Use case |
|---|---|
| P1 | Critical - pages on-call immediately |
| P2 | High - needs attention within the hour |
| P3 | Medium - investigate during business hours |
| P4 | Low - informational, check when convenient |
| P5 | Info - logging/tracking only |

## Create Workflow

1. Ask the user what they want to alert on (logs, metrics, traces)
2. Ask for priority (P1–P5)
3. Build the JSON payload with `alertDefProperties` - use the **API wire format** (see `references/alert-schemas.md` for all enum values)
4. **Tip:** use `cx alerts get <existing-id> -o json` to get a working template, modify it, and pipe into create
5. Create using: `echo '<json>' | cx alerts create` or `cx alerts create --from-file alert.json`
6. Verify with `cx alerts list --name "<alert name>"`

**Important structural note:** The `type` field is a **string enum** (e.g. `"ALERT_DEF_TYPE_LOGS_THRESHOLD"`), and the alert type config (e.g. `"logsThreshold": {...}`) is a **sibling** field at the same level - NOT nested inside `type`.

### Example: Logs Threshold Alert

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
        }
      }]
    }
  }
}
```

### Example: Metric Threshold Alert

```json
{
  "alertDefProperties": {
    "name": "CPU Usage Critical",
    "priority": "ALERT_DEF_PRIORITY_P1",
    "type": "ALERT_DEF_TYPE_METRIC_THRESHOLD",
    "enabled": true,
    "metricThreshold": {
      "metricFilter": { "promql": "avg(cpu_usage_percent)" },
      "rules": [{
        "condition": {
          "conditionType": "METRIC_THRESHOLD_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED",
          "threshold": 90,
          "ofTheLast": { "dynamicDuration": "5m" },
          "forOverPct": 100
        }
      }]
    }
  }
}
```

### Example: Logs Immediate Alert

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
      }
    }
  }
}
```

## Investigation Workflow

### Find firing alerts

```bash
# List all alerts and look for ALERTING status
cx alerts list -o json | jq '.[] | select(.status == "ALERTING")'

# Filter by name
cx alerts list --name "error"
```

### Inspect a specific alert

```bash
cx alerts get <alert-id>
cx alerts get <alert-id> -o json
```

### Disable a noisy alert (temporary mute)

```bash
cx alerts disable <alert-id>
# Later, re-enable:
cx alerts enable <alert-id>
```

## Suppression Rules

Manage alert suppression rules that mute alerts during maintenance windows or known noisy periods.

| Command | Purpose |
|---|---|
| `cx alerts suppression-rules list` | List all suppression rules |
| `cx alerts suppression-rules get <id>` | Get a suppression rule by ID |
| `cx alerts suppression-rules create --from-file` | Create a suppression rule |
| `cx alerts suppression-rules update --from-file` | Update a suppression rule |
| `cx alerts suppression-rules delete <id>` | Delete a suppression rule |

```bash
# List suppression rules
cx alerts suppression-rules list -o json

# Create from template
cx alerts suppression-rules get <existing-id> -o json > suppression-rule.json
# Edit suppression-rule.json
cx alerts suppression-rules create --from-file suppression-rule.json
```

## Key Principles

- **Always ask for priority** (P1–P5) when creating alerts - never assume
- **Use `--name` filter** for large accounts with many alerts
- **Use `-o json` with `jq`** for filtering and transformation
- **Use `--from-file -`** to pipe JSON from stdin when constructing alerts programmatically
- **Verify after create** - always list or get the alert after creation to confirm
- **Disable, don't delete** - prefer disabling alerts over deletion for auditability

---

## Additional Resources

### Reference Files

- **[`references/alert-schemas.md`](references/alert-schemas.md)** - Complete JSON schema reference for all 12 alert types: field names, enum values (condition types, time windows, filter operations), common sub-objects (logs filter, tracing filter, notification groups, activity schedules), and important gotchas
- **[`references/dataprime-reference.md`](references/dataprime-reference.md)** - DataPrime query language reference for log-based and span-based alert conditions (filter syntax, operators, severity values)
- **[`references/logs-querying.md`](references/logs-querying.md)** - Log data model, field discovery, and query patterns for building log alert conditions
- **[`references/promql-guidelines.md`](references/promql-guidelines.md)** - PromQL reference for metric-based alert conditions (counters, gauges, histograms, threshold patterns)
- **[`references/spans-querying.md`](references/spans-querying.md)** - Span data model, duration units, and query patterns for building tracing alert conditions

### Related Skills

- **`cx-incident-management`** - incident triage workflows that involve alerts, SLO monitoring, and notification verification
- **`cx-observability-setup`** - setting up notification routing and webhook integrations for alerts
- **`cx-telemetry-querying`** - investigate the telemetry behind a firing alert
