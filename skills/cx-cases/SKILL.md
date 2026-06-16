---
name: cx-cases
description: >
  Triage and manage Coralogix Cases with the `cx cases` CLI — e.g. acknowledge,
  assign, resolve, or re-prioritize a case, or inspect its event timeline or
  notification deliveries.
metadata:
  version: "0.1.0"
---

# Cases Management Skill

A **Case** groups related alert events into one investigation unit with a status,
priority, category, and assignee. Use this skill to inspect cases and drive them
through their lifecycle (active → acknowledged → resolved → closed).

## CLI Commands

| Command | Purpose |
|---|---|
| `cx cases get <id>` | Get a single case by ID |
| `cx cases update <id> [--title] [--resolution-reason]` | Update mutable fields |
| `cx cases comment <id> --text <text>` | Add a comment to the case timeline |
| `cx cases assign <id> --user <email>` | Assign a case (email, or raw user ID) |
| `cx cases unassign <id>` | Remove the assignee |
| `cx cases acknowledge <id>` | Acknowledge (signals you're working it; stops re-notification) |
| `cx cases unacknowledge <id>` | Remove the acknowledgment |
| `cx cases resolve <id> --reason <text>` | Resolve a case (irreversible — see below) |
| `cx cases close <id>` | Close a case (terminal) |
| `cx cases set-priority <id> --priority <P1..P5>` | Override the computed priority |
| `cx cases clear-priority <id>` | Remove a priority override |
| `cx cases events list <case-id>` | Event timeline (status changes, comments, assignments) |
| `cx cases events get <event-id>` | A single event — drill in, e.g. to expand a comment thread |
| `cx cases notifications <case-id> [<case-id> ...]` | Notification deliveries (connector, status, time) |

## Case Lifecycle

```
PENDING_ACTIVATION ──► ACTIVE ◄────────► ACKNOWLEDGED
                         │ ╲                │ ╲
                         │  ╲               │  ╲
                         ▼   ╲              ▼   ╲
                      CLOSED  ╲──► RESOLVED ◄─── (from ACK)
                                       │
                                       ▼
                                    CLOSED  (terminal)
```

| From state | Allowed transitions | Notes |
|---|---|---|
| `PENDING_ACTIVATION` | → `ACTIVE` | System-driven activation; not user-controllable |
| `ACTIVE` | → `ACKNOWLEDGED`, `RESOLVED`, `CLOSED` | Ack is optional; for a false alarm, `close` directly (skip `resolve`) |
| `ACKNOWLEDGED` | → `ACTIVE`, `RESOLVED`, `CLOSED` | The only "back" transition: `unacknowledge` returns it to `ACTIVE` |
| `RESOLVED` | → `CLOSED` only | **Irreversible** — cannot reopen to `ACTIVE`/`ACKNOWLEDGED` |
| `CLOSED` | (none) | **Terminal** |

Categories: `AVAILABILITY` or `SECURITY`. Priorities: `P1` (highest) → `P5`.

## Triage Workflow

1. **Inspect** — `cx cases get <id>`. The payload includes `groupings`,
   `labels`, `impactedEntities`, `kpiBreaches`, `aiSummary`, and both
   `priorityDetails.system` (computed) and `priorityDetails.override` (user-set).
2. **Investigate** — Pull the underlying telemetry by querying the alert's DataPrime / PromQL to find root cause before acting.
   Optionally export the investigation via `cx olly` or pull the case's `impactedEntities` / `groupings` to confirm the impact. 
   See the `cx-telemetry-querying` skill.
3. **Claim** — `cx cases assign <id> --user you@example.com` then
   `cx cases acknowledge <id>`.
4. **Record findings** — `cx cases comment <id> --text "<note>"` to leave
   investigation notes on the timeline (root cause, links, next steps) as you go.
   Comments appear as `comment` events in `cx cases events list`.
5. **Resolve or close** — see below.
6. **Re-prioritize** if impact differs from the computed value —
   `cx cases set-priority <id> --priority P1` / `clear-priority`. Only possible
   while the case is still open; priority cannot be overridden once a case is
   `RESOLVED` or `CLOSED`.

### Resolving

Resolution is **irreversible** (a `RESOLVED` case can only move to `CLOSED`), so
the CLI requires both a reason and a confirmation:

- Pass `--reason "<text>"` — a one-line postmortem (root cause, what fixed it,
  follow-up) visible to teammates in the timeline. Use `--no-reason` only when a
  reason genuinely doesn't apply.
- In agent / non-interactive mode, also pass `--yes`; without it the command
  refuses and must be handed to the user to run interactively.

If uncertain, stay in `ACKNOWLEDGED` (reversible via `unacknowledge`) until
confident. For non-resolution edits (title, post-hoc postmortem link), use
`cx cases update`.

## Bulk Operations

There are no bulk endpoints. To act on many cases, pipe IDs through a loop, e.g.
`... | jq -r '.[].id' | xargs -I {} cx cases acknowledge {}`.

## Key Principles

- **Use emails, never user IDs** — for `assign --user` and in all output.
- **`resolve` is irreversible and `close` is terminal** — confirm before resolving;
  for false alarms `close` from `ACTIVE` directly.
- **Always supply a resolution reason** unless `--no-reason` truly applies.
- **`P1`-style shorthand** is accepted anywhere a priority/status/category is expected.
- **Multi-profile fan-out** with `-p <profile>` (repeatable) for cross-environment triage.

## References

- Case analytics: [`references/case-analytics.md`](references/case-analytics.md)
- Single case investigation: [`references/single-case.md`](references/single-case.md)

## Related Skills

- **`cx-alerts`** — the alert definitions behind the events grouped into a case.
- **`cx-telemetry-querying`** — pivot from a case's impacted entities into logs/spans/metrics.
