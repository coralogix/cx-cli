---
name: cx-cases
description: >
  This skill should be used when the user asks to "triage a case", "acknowledge a
  case", "assign a case", "resolve a case", "close a case", "comment on a case",
  "re-prioritize a case", "check case status", "see a case timeline", "check case
  notifications", "create a case", "open a case", "link cases", "find the parent
  case", "find child cases", "find cases Olly opened", "find cases from a scheduled
  task", or wants to inspect or drive Coralogix Cases through their lifecycle using
  the cx CLI.
metadata:
  version: "0.2.0"
---

# Cases Management Skill

A **Case** groups related indicators — usually alert events, sometimes an external
source — into one investigation unit with a status, priority, category, and
assignee. Use this skill to inspect cases and drive them through their lifecycle
(active → acknowledged → resolved → closed).

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

## What the CLI cannot do

Tell the user plainly and point at the alternative — do not improvise a command
that does not exist.

| Not available | Why, and what to do instead |
|---|---|
| **Creating a case** | Cases are opened by the Cases service from an indicator — an alert event, an Error Tracking issue, or an Olly scheduled task. Create exists only on the *internal* API, so there is no `cx cases create`. To get a case for something: ask Olly to open one (`cx olly`, which can), fix the alert that should have caught it (`cx-alerts`), or open one in the Cases UI. |
| **Listing / searching cases** | `cx cases list` was removed. Case discovery lives in the dataset: query `system/labs.cases.state_updates` with `cx dataprime`. See [`references/case-analytics.md`](references/case-analytics.md). |
| **Linking / unlinking cases** | Parent-child links are created in the Cases UI or through the API, not the CLI. You can still *read* the hierarchy from the dataset — see "Case relationships" below. |
| **Muting a case** | No CLI verb. Mute in the UI. Muting stops notifications without changing status, which is the honest alternative to resolving work nobody has finished. |
| **Changing labels** | Labels are fixed when the case is created. `cx cases update` patches only the title and resolution reason. |
| **Reopening** | `RESOLVED` → `CLOSED` only. A recurrence of the same underlying indicator reopens the case automatically; a person cannot. |

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

## Case relationships (parent / child)

Cases can be linked into a **one-level** parent-child hierarchy: the parent is the
incident of record and children hang under it. A case has at most one parent, is
either a parent or a child but never both, and a parent has at most 100 children —
so there are no grandparents and no cycles. There is no "related" or "duplicate"
link type.

Linking a case as a child **mutes it** by default, so the parent becomes the single
notifying case. Expect a child to be quiet; that is the design, not a delivery
failure.

`cx cases get <id>` returns the case's `relationship`: absent for a standalone case,
`{"type": "PARENT"}` for a parent, `{"type": "CHILD", "parentCaseId": "<uuid>"}` for
a child. A parent does not inline its children — to list them, see
[Case relationships](references/case-analytics.md#case-relationships-parent--child)
in the analytics reference.

When triaging a child, read the parent first — the investigation usually lives
there. When resolving a parent, check its children: they do not resolve with it.

## Cases opened by Olly

A case has **no top-level source field**. Provenance lives on its indicators, so a
case Olly opened carries a generic indicator with
`indicatorType == "OLLY_SCHEDULED_TASK"` and a free-form `metadata` object naming the
task and run it came from. That also means `cx cases` cannot filter for Olly's cases
— see
[Generic indicators](references/case-analytics.md#generic-indicators--non-alert-sources-including-olly)
in the analytics reference for the query.

Triage one the same way as any other case, with one addition: open the run link in
the indicator's `metadata` to see the evidence Olly based the case on, and judge that
evidence rather than taking the case's own summary at face value.

The indicator's `externalId` is the dedup key. One unresolved case exists per key, and
a recurrence reopens the resolved case rather than forking a new one — so a case with
a non-null `lastReactivatedAt` is a repeat, which is usually the more interesting
signal than the current occurrence.

## Bulk Operations

There are no bulk endpoints. To act on many cases, pipe IDs through a loop, e.g.
`... | jq -r '.[].id' | xargs -I {} cx cases acknowledge {}`.

## Key Principles

- **Use emails, never user IDs** — for `assign --user` and in all output.
- **`resolve` is irreversible and `close` is terminal** — confirm before resolving;
  for false alarms `close` from `ACTIVE` directly.
- **Always supply a resolution reason** unless `--no-reason` truly applies.
- **`P1`-style shorthand** is accepted anywhere a priority/status/category is expected.
- **Acknowledging means a person is on it** — escalation, PagerDuty and ServiceNow
  all read that fact, so never acknowledge on someone's behalf to quiet a case.
  Assign it, or mute it in the UI.
- **Prefer muting over resolving** when the work is real but nobody is on it. A
  resolved case is a claim the system is healthy.
- **A case has no source field** — provenance is on its indicators. See
  "Cases opened by Olly".
- **Multi-profile fan-out** with `-p <profile>` (repeatable) for cross-environment triage.
- **Link to a specific case** — build `<base>/cases?id=<case_id>`, where
  `<base>` is the console URL already seen in a `View in Coralogix:
  <base>/...` line printed by any `cx cases` command this session — never
  fabricate `<base>` yourself.

## References

- Case analytics: [`references/case-analytics.md`](references/case-analytics.md)
- Single case investigation: [`references/single-case.md`](references/single-case.md)

## Related Skills

- **`cx-alerts`** — the alert definitions behind the events grouped into a case.
- **`cx-slos`** — the SLO definitions whose breaches drive reliability cases.
- **`cx-telemetry-querying`** — pivot from a case's impacted entities into logs/spans/metrics.
