---
name: cx-cost-optimization
description: >
  Use this skill when the user asks to "check data usage", "list TCO policies",
  "reduce Coralogix costs", "optimize observability spend", "lower our logging bill",
  "data budget exceeded", "TCO policy", "retention tier", "archive storage", "ingestion costs",
  "frequent search vs archive", "why is our bill so high", "spending too much on logs",
  "data retention settings", "cost analysis", "usage breakdown",
  "optimize log volume", "control data ingestion", "archive cold data",
  "billing units", "plan consumption", "daily plan", "overage", "PAYG",
  "usage anomaly", "usage trend", "cx_data_usage_units",
  or wants to investigate, analyze, or reduce Coralogix data costs.
metadata:
  version: "0.1.0"
---

# Cost Optimization Skill

Use this skill when investigating or reducing Coralogix data costs. It covers the full cost management lifecycle: measuring current spend, reviewing TCO policies, adjusting retention periods, and configuring archive storage for cold data.

---

## CLI Commands

| Command | Subcommands | Purpose |
|---|---|---|
| `cx usage` | `summary`, `daily`, `logs-count`, `spans-count`, `export-status` | Measure current data consumption |
| `cx tco` | `list`, `get`, `create`, `update`, `delete`, `reorder`, `test`, `settings`, `settings-update` | Manage TCO (Total Cost of Ownership) policies |
| `cx retentions` | `list`, `update`, `activate`, `status` | Manage data retention periods |
| `cx archive logs` | `get`, `set` | Configure logs archive target |
| `cx archive metrics` | `get`, `create`, `update`, `enable`, `disable`, `validate` | Configure metrics archive storage |
| `cx metrics query` | `<promql>` (positional), `--time` | Query billing and usage metrics via PromQL (instant) |
| `cx metrics query-range` | `<promql>` (positional), `--start`/`--end` | Query billing and usage metrics via PromQL (range) |

Key flags:
- All commands support `-o json` for structured output and `-p <profile>` for profile selection
- `cx usage daily` accepts `--type processed-gbs|units|evaluation-tokens` and `--start`/`--end` time filters
- `cx usage summary` accepts `--start`/`--end` time filters
- `cx usage logs-count` and `cx usage spans-count` accept `--start`/`--end` time filters, defaulting to the last 24h, plus `--resolution` (default `1h`), `--subsystem-aggregation`, `--application-aggregation`, and repeated `--param KEY=VALUE` for API filter query params
- Data usage summary and count endpoints are documented as newline-delimited JSON over `Accept: text/event-stream`; the CLI handles that transport and normalizes count chunks into `.result.logsCount[]` or `.result.spansCount[]`.
- `cx tco create/update`, `cx retentions update`, `cx archive logs set`, `cx archive metrics create/update/validate` use `--from-file <path>` (or `-` for stdin)

---

## Cost Investigation Workflow

Follow these steps to diagnose and reduce costs:

### Step 1: Measure Current Usage

```bash
cx usage summary -o json
cx usage summary --start now-30d -o json
cx usage daily --type processed-gbs --start now-7d -o json
cx usage logs-count --start now-7d --end now -o json
cx usage spans-count --start now-7d --end now -o json
```

Identify which data types consume the most volume. Use `jq` to sort:

```bash
cx usage summary -o json | jq '[.[] | {name, daily_avg: .avg_daily_gb}] | sort_by(.daily_avg) | reverse'
```

### Step 2: Review TCO Policies

```bash
cx tco list -o json
cx tco settings -o json
```

TCO policies control which logs go to Frequent Search (expensive, fast) vs. Archive (cheap, slower). Check if high-volume, low-value logs are on Frequent Search:

```bash
cx tco list -o json | jq '.[] | select(.priority == "LOW") | {name, application, subsystem, archive_retention}'
```

### Step 3: Check Retention Settings

```bash
cx retentions list -o json
cx retentions status -o json
```

Long retention periods increase storage costs. Identify indices with unnecessarily long retention.

### Step 4: Check Archive Configuration

```bash
cx archive logs get -o json
cx archive metrics get -o json
```

Verify that archive storage is configured for cold data. If no archive is set up, that's a cost-saving opportunity.

### Step 5: Recommend Optimizations

Based on findings, recommend changes in priority order (highest impact first).

---

## Common Optimization Patterns

| Symptom | Diagnosis Command | Optimization |
|---|---|---|
| High-volume low-value logs | `cx usage summary -o json` | Move to archive tier via `cx tco create --from-file policy.json` |
| Long retention on cold data | `cx retentions list -o json` | Reduce retention with `cx retentions update --from-file` |
| No cold storage configured | `cx archive logs get -o json` | Enable archive with `cx archive logs set --from-file --yes` (after user approval) |
| Expensive metrics not queried | `cx archive metrics get -o json` | Enable metrics archiving with `cx archive metrics create --from-file --yes` (after user approval) |

---

## jq Examples

### Usage Analysis

```bash
# Top consumers by daily volume
cx usage summary -o json | jq '[.[] | {name, daily_avg: .avg_daily_gb}] | sort_by(.daily_avg) | reverse | .[0:10]'

# Daily trend for the past week
cx usage daily --type processed-gbs --start now-7d -o json | jq '[.[] | {date, gb: .processed_gbs}]'

# Total logs and spans counts
cx usage logs-count --start now-7d --end now -o json | jq '[.result.logsCount[]?.logsCount | tonumber] | add // 0'
cx usage spans-count --start now-7d --end now -o json | jq '[.result.spansCount[]? | ((.successSpanCount | tonumber) + (.errorSpanCount | tonumber) + (.lowSuccessSpanCount | tonumber) + (.lowErrorSpanCount | tonumber) + (.mediumSuccessSpanCount | tonumber) + (.mediumErrorSpanCount | tonumber))] | add // 0'
```

### TCO Policy Analysis

```bash
# Policies routing to archive tier
cx tco list -o json | jq '[.[] | select(.archive_retention != null)]'

# Policies by priority
cx tco list -o json | jq 'group_by(.priority) | map({priority: .[0].priority, count: length})'

# Test if a log pattern matches a policy
cx tco test --from-file test-definition.json -o json
```

### Retention Review

```bash
# All retention settings
cx retentions list -o json | jq '.[]'

# Check if retention is active
cx retentions status -o json
```

### Archive Status

```bash
# Logs archive configuration
cx archive logs get -o json | jq '{active: .active, bucket: .bucket}'

# Metrics archive configuration
cx archive metrics get -o json | jq '{enabled: .enabled, bucket: .bucket}'
```

---

## Applying Changes

**IMPORTANT: NEVER pass `--yes` without explicit user approval.** All write operations across archive, TCO, and retentions require interactive confirmation and the `--yes` flag to execute non-interactively. Before executing any write operation, describe the exact change to the user and wait for their approval before passing `--yes`.

**Read-only mode:** Use `--read-only` (or `CX_READ_ONLY=1`) to safely explore cost data without risk of accidental writes. All query commands (usage, tco list/get, retentions list, archive get) work normally in read-only mode.

**Agent mode:** When running inside an AI agent, cx fails fast on write operations instead of hanging on a stdin prompt. Get user confirmation first, then re-run with `--yes`.

When modifying TCO policies, retention, or archive:

1. **Template from existing:** Get the current configuration as JSON, modify it, then apply:
   ```bash
   cx tco get <policy-id> -o json > policy.json
   # Edit policy.json
   cx tco update --from-file policy.json
   ```

2. **Verify after changes:** Re-run the diagnosis commands to confirm the change took effect.

3. **TCO policy ordering matters:** Use `cx tco reorder --from-file` to set priority order. Policies are evaluated top-to-bottom; the first match wins.

---

## Metrics-Based Cost Analysis

The `cx usage` API gives summaries, but for billing-accurate analysis, anomaly detection, and breakdown by pillar/feature, query the customer metrics exporter via PromQL.

### Key Metrics

| Metric | Meaning | Query suffix |
|---|---|---|
| `cx_data_usage_units` | Daily billable usage in units (canonical billing metric) | No `_total` |
| `cx_data_plan_units_per_day` | Current daily plan quota in units (snapshot) | No `_total` |
| `cx_data_usage_payg_units` | Daily overage/PAYG usage in units | No `_total` |
| `cx_data_usage_total` | Processed data size in bytes | `_total` |
| `cx_data_usage_tokens_total` | AI evaluation tokens | `_total` |
| `cx_data_usage_samples_total` | Processed metric samples | `_total` |

### Concept-to-Metric Mapping

- **Billing / plan usage / consumption** -> `cx_data_usage_units` + `cx_data_plan_units_per_day`
- **Processed bytes / data volume** -> `cx_data_usage_total`
- **AI evaluation tokens** -> `cx_data_usage_tokens_total`
- **Metric samples** -> `cx_data_usage_samples_total`
- **Overage / PAYG** -> `cx_data_usage_payg_units`

### Common PromQL Queries

```bash
# Today's billable units consumed so far
cx metrics query 'sum(cx_data_usage_units)' --time now -o json

# Units breakdown by pillar
cx metrics query 'sum by (pillar) (cx_data_usage_units)' --time now -o json

# Daily plan quota
cx metrics query 'cx_data_plan_units_per_day' --time now -o json

# Plan consumption percentage
cx metrics query '100 * sum(cx_data_usage_units) / cx_data_plan_units_per_day' --time now -o json

# Units by feature group
cx metrics query 'sum by (feature_group_id) (cx_data_usage_units)' --time now -o json

# PAYG overage (if any)
cx metrics query 'cx_data_usage_payg_units' --time now -o json
```

### UTC-Day Bucketing Rules

All usage metrics accumulate from UTC midnight and reset at `00:00 UTC`:
- An instant query during the day returns "today so far"
- For completed-day totals, use the last sample before midnight
- Never subtract values across a UTC midnight boundary
- For weekly/monthly analysis, derive completed daily totals first, then roll up
- Exclude the current partial UTC day when computing trends or averages

### Anomaly Detection

When investigating usage anomalies:
1. Compare completed UTC days (exclude current partial day)
2. Break down by: `measurement_type` -> `pillar` -> `entity_type` -> `priority` -> `feature_group_id` -> `application_name` -> `subsystem_name`
3. Prefer same-weekday comparisons for seasonal traffic
4. Use `cx_data_usage_units` for billing anomalies, `cx_data_usage_total` for volume anomalies

### Breakdown Labels

Usage metrics support these grouping dimensions: `pillar`, `entity_type`, `priority`, `measurement_type`, `feature_group_id`, `feature_id`, `application_name`, `subsystem_name`.

---

## Key Principles

- **Measure before changing** - always run usage/summary commands before modifying policies
- **Use `-o json` with jq** - structured output enables precise analysis
- **Verify changes** - re-query after every modification to confirm it took effect
- **Multi-profile awareness** - use `-p <profile>` or `--all-profiles` to compare costs across environments
- **Template from existing** - get current config as JSON before creating or updating
- **TCO is the biggest lever** - moving logs from Frequent Search to Archive tier has the largest cost impact

---

## Related Skills

- **`cx-telemetry-querying`** - investigate what data is being ingested (query logs, metrics, and spans to identify high-volume sources)
