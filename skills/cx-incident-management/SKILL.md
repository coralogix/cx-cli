---
name: cx-incident-management
description: >
  Use this skill when the user asks to "investigate incident", "triage this alert",
  "what's firing", "who got paged", "incident response", "check incident status",
  "SLO breaching", "error budget burned", "check service level", "SLI status",
  "who was notified", "check notification delivery", "verify alert routing",
  "MTTR", "incident severity", "error budget", "burn rate",
  "acknowledge incident", "resolve incident", "production incident",
  "what alerts are active", "incident timeline", "on-call triage",
  or wants to triage, manage, or respond to incidents using alerts, SLOs, and notifications.
metadata:
  version: "0.1.0"
---

# Incident Management Skill

Use this skill as the gateway for incident triage, SLO monitoring, and notification verification. It orchestrates the full triage workflow - from detection through resolution - and cross-references `cx-alerts` for deep alert management and `cx-telemetry-querying` for root cause investigation.

---

## CLI Commands

| Command | Subcommands | Purpose |
|---|---|---|
| `cx incidents` | `list`, `get`, `acknowledge`, `resolve`, `close`, `assign`, `unassign`, `events`, `aggregations` | Manage and triage incidents |
| `cx slos` | `list`, `get`, `create`, `update`, `delete` | Monitor and manage SLO definitions |
| `cx alerts` | `list`, `get` | Check which alerts are firing (see `cx-alerts` skill for full alert management) |
| `cx notifications connectors` | `list`, `get` | Verify notification connector configuration |
| `cx notifications routers` | `list`, `get` | Verify notification routing rules |
| `cx notifications presets` | `list`, `get` | Check notification preset templates |
| `cx notifications test` | `connector`, `destination`, `preset`, `routing-condition`, `template-render` | Test notification delivery |

Key flags:
- `cx incidents list` supports repeatable filters: `--status` (TRIGGERED, ACKNOWLEDGED, RESOLVED), `--severity` (INFO, WARNING, ERROR, CRITICAL), `--state` (TRIGGERED, RESOLVED), `--assignee`, `--application-name`, `--subsystem-name`, `--contextual-label key=value`, `--query`, `--muting muted|unmuted`, `--start/--end`, and `--duration-start/--duration-end`
- `cx incidents list` returns at most 100 incidents per profile by default. Use `--limit <n>` for a bounded per-profile result set, `--page-size <n>`/`--page-token <token>` for manual pagination, or `--all` only when you explicitly need every page.
- All commands support `-o json` for structured output and `-p <profile>` for profile selection
- `cx slos create/update` use `--from-file <path>` (or `-` for stdin)

---

## Incident Triage Workflow

### Step 1: Check Active Incidents

```bash
cx incidents list -o json
cx incidents list --status TRIGGERED -o json
cx incidents list --severity CRITICAL -o json
cx incidents list --status TRIGGERED --start now-24h --limit 50 -o json
```

Get an overview of what's happening. Filter by severity for immediate priorities:

```bash
cx incidents list --severity CRITICAL --limit 50 -o json | jq '[.[] | {id, name, state, severity, created_at}]'
```

### Step 2: Get Incident Details

```bash
cx incidents get <incident-id> -o json
cx incidents events --incident-id <incident-id> -o json
```

Review the incident timeline and related events to understand scope and progression.

### Step 3: Check Related Alerts

```bash
cx alerts list -o json
```

Find which alerts are currently firing. For deep alert inspection, switch to the `cx-alerts` skill.

```bash
cx alerts list -o json | jq '[.[] | select(.is_active == true) | {id, name, severity, last_triggered}]'
```

### Step 4: Review SLO Status

```bash
cx slos list -o json
cx slos get <slo-id> -o json
```

Check if SLOs are breaching or error budgets are burned:

```bash
cx slos list -o json | jq '[.[] | {name, status, remaining_budget_percentage}]'
```

### Step 5: Verify Notifications

```bash
cx notifications connectors list -o json
cx notifications routers list -o json
cx notifications presets list -o json
```

Confirm the right people were notified through the correct channels.

### Step 6: Pivot to Root Cause

Switch to the `cx-telemetry-querying` skill to investigate the underlying cause using logs, traces, and metrics.

---

## Incident Actions

### Acknowledge

```bash
cx incidents acknowledge <incident-id>
cx incidents acknowledge <id1> <id2> <id3>
```

### Resolve

```bash
cx incidents resolve <incident-id>
cx incidents resolve <id1> <id2> <id3>
```

### Assign

```bash
cx incidents assign <incident-id> --user-id <user-id>
```

### Close

```bash
cx incidents close <incident-id>
```

---

## SLO Management

### Creating SLOs

Template from an existing SLO:

```bash
cx slos get <existing-slo-id> -o json > slo-template.json
# Edit slo-template.json with new service/threshold
cx slos create --from-file slo-template.json
```

### Monitoring SLO Health

```bash
# All SLOs with their status
cx slos list -o json | jq '[.[] | {name, status, target_percentage, remaining_budget}]'

# SLOs that are breaching
cx slos list -o json | jq '[.[] | select(.status != "OK")]'
```

---

## Notification Debugging

When notifications aren't reaching the right people:

### 1. Check Connectors

```bash
cx notifications connectors list -o json | jq '[.[] | {id, name, type}]'
```

Verify the expected channels (Slack, PagerDuty, email) exist and are configured.

### 2. Check Routers

```bash
cx notifications routers list -o json | jq '[.[] | {id, name, entity_type}]'
```

Verify routing rules map the right alert types to the right connectors.

### 3. Test Notification Delivery

```bash
cx notifications test connector --from-file test-connector.json
cx notifications test destination --from-file test-destination.json
cx notifications test preset --from-file test-preset.json
cx notifications test routing-condition --from-file test-condition.json
```

---

## Incident Aggregations

Get a high-level view of incident patterns:

```bash
cx incidents aggregations -o json
```

Use this to understand incident frequency, MTTR trends, and severity distribution.

---

## Key Principles

- **Triage before deep-dive** - check incidents, alerts, and SLOs before querying telemetry data
- **Check SLO burn rate, not just status** - a slowly burning SLO needs attention before it breaches
- **Verify notification chain end-to-end** - connector exists → router maps correctly → test delivery works
- **Cross-reference with telemetry** - use `cx-telemetry-querying` skill for root cause after triage
- **Acknowledge promptly** - acknowledge incidents to signal ownership and stop re-notifications
- **Use incident events for timeline** - `cx incidents events` shows the full incident lifecycle

---

## Related Skills

- **`cx-alerts`** - deep alert management: creating, updating, and inspecting alert definitions
- **`cx-telemetry-querying`** - root cause investigation using logs, metrics, traces, and RUM
- **`cx-observability-setup`** - configure notification channels and routing for alerts
