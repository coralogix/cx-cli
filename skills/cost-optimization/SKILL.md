---
name: cost-optimization
description: >
  Use this skill when the user asks to "check data usage", "list TCO policies", "view quotas",
  "reduce Coralogix costs", "optimize observability spend", "lower our logging bill",
  "data budget exceeded", "TCO policy", "retention tier", "archive storage", "ingestion costs",
  "frequent search vs archive", "why is our bill so high", "spending too much on logs",
  "data retention settings", "quota rules", "cost analysis", "usage breakdown",
  "optimize log volume", "control data ingestion", "archive cold data",
  or wants to investigate, analyze, or reduce Coralogix data costs.
version: 0.1.0
---

# Cost Optimization Skill

Use this skill when investigating or reducing Coralogix data costs. It covers the full cost management lifecycle: measuring current spend, reviewing TCO policies, adjusting retention periods, setting ingestion quotas, and configuring archive storage for cold data.

---

## CLI Commands

| Command | Subcommands | Purpose |
|---|---|---|
| `cx usage` | `summary`, `daily`, `logs-count`, `spans-count`, `export-status` | Measure current data consumption |
| `cx tco` | `list`, `get`, `create`, `update`, `delete`, `reorder`, `test`, `settings`, `settings-update` | Manage TCO (Total Cost of Ownership) policies |
| `cx retentions` | `list`, `update`, `activate`, `status` | Manage data retention periods |
| `cx quotas` | `get`, `create`, `update`, `delete` | Set ingestion guardrails |
| `cx archive logs` | `get`, `set` | Configure logs archive target |
| `cx archive metrics` | `get`, `create`, `update`, `enable`, `disable`, `validate` | Configure metrics archive storage |

Key flags:
- All commands support `-o json` for structured output and `-p <profile>` for profile selection
- `cx usage daily` accepts `--type processed-gbs|units|evaluation-tokens` and `--start`/`--end` time filters
- `cx usage summary` accepts `--start`/`--end` time filters
- `cx tco create/update`, `cx retentions update`, `cx quotas create/update`, `cx archive logs set`, `cx archive metrics create/update/validate` use `--from-file <path>` (or `-` for stdin)

---

## Cost Investigation Workflow

Follow these steps to diagnose and reduce costs:

### Step 1: Measure Current Usage

```bash
cx usage summary -o json
cx usage summary --start now-30d -o json
cx usage daily --type processed-gbs --start now-7d -o json
cx usage logs-count -o json
cx usage spans-count -o json
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

### Step 4: Review Quota Rules

```bash
cx quotas get -o json
```

Quota rules cap ingestion volume. If there are no quotas and you see burst ingestion, recommend adding guardrails.

### Step 5: Check Archive Configuration

```bash
cx archive logs get -o json
cx archive metrics get -o json
```

Verify that archive storage is configured for cold data. If no archive is set up, that's a cost-saving opportunity.

### Step 6: Recommend Optimizations

Based on findings, recommend changes in priority order (highest impact first).

---

## Common Optimization Patterns

| Symptom | Diagnosis Command | Optimization |
|---|---|---|
| High-volume low-value logs | `cx usage summary -o json` | Move to archive tier via `cx tco create --from-file policy.json` |
| Long retention on cold data | `cx retentions list -o json` | Reduce retention with `cx retentions update --from-file` |
| Burst ingestion spikes | `cx usage daily -o json` | Add quota rules with `cx quotas create --from-file` |
| No cold storage configured | `cx archive logs get -o json` | Enable archive with `cx archive logs set --from-file` |
| Expensive metrics not queried | `cx archive metrics get -o json` | Enable metrics archiving with `cx archive metrics create --from-file` |

---

## jq Examples

### Usage Analysis

```bash
# Top consumers by daily volume
cx usage summary -o json | jq '[.[] | {name, daily_avg: .avg_daily_gb}] | sort_by(.daily_avg) | reverse | .[0:10]'

# Daily trend for the past week
cx usage daily --type processed-gbs --start now-7d -o json | jq '[.[] | {date, gb: .processed_gbs}]'

# Total logs and spans counts
cx usage logs-count -o json | jq '.total_count'
cx usage spans-count -o json | jq '.total_count'
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

### Quota Analysis

```bash
# Current quota rules
cx quotas get -o json | jq '.rules // empty'
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

When modifying TCO policies, retention, quotas, or archive:

1. **Template from existing:** Get the current configuration as JSON, modify it, then apply:
   ```bash
   cx tco get <policy-id> -o json > policy.json
   # Edit policy.json
   cx tco update --from-file policy.json
   ```

2. **Verify after changes:** Re-run the diagnosis commands to confirm the change took effect.

3. **TCO policy ordering matters:** Use `cx tco reorder --from-file` to set priority order. Policies are evaluated top-to-bottom; the first match wins.

---

## Key Principles

- **Measure before changing** — always run usage/summary commands before modifying policies
- **Use `-o json` with jq** — structured output enables precise analysis
- **Verify changes** — re-query after every modification to confirm it took effect
- **Multi-profile awareness** — use `-p <profile>` or `--all-profiles` to compare costs across environments
- **Template from existing** — get current config as JSON before creating or updating
- **TCO is the biggest lever** — moving logs from Frequent Search to Archive tier has the largest cost impact

---

## Related Skills

- **`telemetry-querying`** — investigate what data is being ingested (understand usage before cutting)
- **`query-logs`** — query logs to identify high-volume sources
- **`metrics-query`** — check metric cardinality and usage
