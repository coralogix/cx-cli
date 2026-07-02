---
name: cx-slos
description: >
  Manage Coralogix SLO (Service Level Objective) definitions with the `cx slos`
  CLI — list and inspect SLOs, check whether targets and error budgets are
  healthy, and create, update, or delete SLO definitions from JSON. Use when the
  user asks to "list SLOs", "check SLO status", "is an SLO breaching", "error
  budget", "create an SLO", "update an SLO", "delete an SLO", or "service level
  objective".
metadata:
  version: "0.1.0"
---

# SLO Management Skill

An **SLO** (Service Level Objective) defines a reliability target for a service —
e.g. "99.9% of requests succeed over 28 days". Use this skill to inspect SLO
definitions, judge whether they are healthy, and manage their lifecycle.

## CLI Commands

| Command | Purpose |
|---|---|
| `cx slos list` | List all SLO definitions |
| `cx slos get <id>` | Get a single SLO by ID |
| `cx slos create --from-file <path>` | Create an SLO from a JSON definition [requires `--yes`] |
| `cx slos update --from-file <path>` | Replace an SLO definition from JSON [requires `--yes`] |
| `cx slos delete <id>` | Delete an SLO [requires `--yes`] |

- All commands support `-o json` for structured output and `-p <profile>` (repeatable) for multi-profile fan-out.
- `create`/`update` read from `--from-file <path>`, or `-` for stdin (the default).
- `create`, `update`, and `delete` are write operations and require `--yes` in non-interactive / agent mode.

## SLO Definition

The fields surfaced on every SLO:

| Field | Meaning |
|---|---|
| `name` | Human-readable SLO name |
| `description` | Optional description |
| `targetThresholdPercentage` | The objective, e.g. `99.9` |
| `sloType` | `SLO_TYPE_REQUEST` (request-based) or `SLO_TYPE_WINDOW` (window-based) |
| `sloTimeFrame` | Rolling window, e.g. `SLO_TIME_FRAME_7_DAYS`, `SLO_TIME_FRAME_28_DAYS` |
| `productType` | The pillar the SLO is computed from, e.g. `SLO_PRODUCT_TYPE_APM` |

## Monitoring SLO Health

```bash
# All SLOs with their target and window
cx slos list -o json | jq '[.[] | {name, targetThresholdPercentage, sloType, sloTimeFrame}]'

# Inspect a single SLO in full
cx slos get <slo-id> -o json
```

Compare the live attainment against `targetThresholdPercentage` to judge whether
the SLO is healthy or burning error budget. A **request-based** SLO measures the
fraction of good events; a **window-based** SLO measures the fraction of good time
windows. After triage, pivot to the `cx-telemetry-querying` skill to find the root
cause of a breach in logs, spans, or metrics.

## Creating and Updating SLOs

The safest way to author an SLO is to round-trip an existing one — template from the
exact field shape that `get` returns rather than hand-writing JSON:

```bash
# Template from an existing SLO, edit, then create
cx slos get <existing-slo-id> -o json > slo.json
# Edit slo.json: change name, targetThresholdPercentage, sloType, sloTimeFrame, etc.
cx slos create --from-file slo.json --yes

# Replace an existing definition
cx slos update --from-file slo.json --yes
```

`update` and `delete` report the alert IDs they affect (`effectedSloAlertIds`), so
review that list — changing or removing an SLO can disable the alerts attached to
it.

## Key Principles

- **Check attainment vs. target, not just existence** — an SLO at 99.91% against a
  99.9% target has almost no error budget left and needs attention before it breaches.
- **Round-trip definitions** — template create/update payloads from `cx slos get`
  rather than hand-writing JSON, to keep the exact field shape the API expects.
- **`create`/`update`/`delete` need `--yes`** in agent / non-interactive mode.
- **Watch affected alerts** — `update`/`delete` return `effectedSloAlertIds`; deleting
  an SLO can silence its alerts.
- **Multi-profile fan-out** with `-p <profile>` (repeatable) to compare SLOs across environments.

## Related Skills

- **`cx-alerts`** — the alert definitions that fire when an SLO's error budget burns.
- **`cx-telemetry-querying`** — investigate the logs, spans, and metrics behind an SLO breach.
- **`cx-cases`** — triage the cases that group the alert events raised against a service.
