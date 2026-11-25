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
| `cx slos status <id> [<id>...]` | Compliance and remaining error budget for one or more SLOs |
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

```bash
# Status over the SLO's own window, then over the last 4 hours
cx slos status <slo-id>
cx slos status <slo-id> <slo-id> --last 4h -o json

# Explicit window, one permutation, what-if target
cx slos status <slo-id> --from now-7d --to now-1d
cx slos status <slo-id> --grouping-key service,api --target 99.9
```

`status` returns `lowest_compliance_percentage` and
`lowest_remaining_error_budget_percentage`, both from 0 to 100 — the lowest value
across the matching permutations — plus the window actually used (`effective_from`,
`effective_to`) and `complete_data`.

- `--last 4h` or `--from`/`--to` set the window; times are cut to the whole minute and
  the server rejects windows over its limit. `--last` cannot be combined with `--from`/`--to`.
- `--grouping-key label,value` (repeatable) narrows to permutations with that label
  value; the label must be one of the SLO's grouping labels.
- If any SLO fails (unknown id, bad label, window too long), the command fails and
  prints nothing; the error names the first failed SLO. Fix the input and rerun.

**`0` and `0` has two meanings.** Either the window has no events (or the label value
is unknown), or some permutation failed every event, which is a real breach.
`complete_data` does not tell them apart. Before reporting a breach, count the SLO's
events: use `effective_to` as `--time` and the window length as the range.

```bash
# Request-based SLO (type REQUEST in `cx slos list`)
cx metrics query 'sum(sum_over_time(cx:slo:request:total_events{slo_id="<slo-id>"}[4h]))' --time <effective_to>

# Window-based SLO (type WINDOW)
cx metrics query 'sum(count_over_time(cx:slo:window{slo_id="<slo-id>"}[4h]))' --time <effective_to>
```

- Empty result or `0`: no events. Report "no data", not a breach.
- Positive number: events exist, so the zero is real and the SLO is breached.
- With `--grouping-key label,value`, add `label="value"` to the selector.

Compare `lowest_compliance_percentage` against `target` from `cx slos list -o json`
(both are percentages) to judge whether the SLO is healthy or burning error budget. A **request-based** SLO measures the
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
