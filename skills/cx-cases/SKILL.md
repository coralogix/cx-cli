---
name: cx-cases
description: >
  This skill should be used when the user asks to "triage a case", "acknowledge a
  case", "assign a case", "resolve a case", "close a case", "comment on a case",
  "re-prioritize a case", "check case status", "see a case timeline", "check case
  notifications", "create a case", "open a case", "mute a case", "link cases",
  "make this a child of", "unlink a case", "find the parent case", "find child
  cases", "find where a case came from", or wants to inspect or drive Coralogix Cases through their lifecycle using
  the cx CLI.
metadata:
  version: "0.3.0"
---

# Cases Management Skill

A **Case** groups related indicators — usually alert events, sometimes an external
source — into one investigation unit with a status, priority, category, and
assignee. Cases can also be **created manually** with no indicator behind them.
Use this skill to open cases on demand, inspect them, and drive them through their
lifecycle (active → acknowledged → resolved → closed).

## CLI Commands

| Command | Purpose |
|---|---|
| `cx cases create --title <t> --priority <P1..P5> --description <text> [--label k=v ...] [--assignee <email>] [--status ACTIVE\|ACKNOWLEDGED]` | Create a case manually — see "Creating a case" |
| `cx cases get <id>` | Get a single case by ID |
| `cx cases update <id> [--title] [--resolution-reason] [--description]` | Update mutable fields; `--description` replaces the whole description — see "Updating the description" |
| `cx cases comment <id> --text <text>` | Add a comment to the case timeline |
| `cx cases assign <id> --user <email>` | Assign a case (email, or raw user ID) |
| `cx cases unassign <id>` | Remove the assignee |
| `cx cases acknowledge <id>` | Acknowledge (signals you're working it; stops re-notification) |
| `cx cases unacknowledge <id>` | Remove the acknowledgment |
| `cx cases resolve <id> --reason <text>` | Resolve a case (irreversible — see below) |
| `cx cases close <id>` | Close a case (terminal) |
| `cx cases set-priority <id> --priority <P1..P5>` | Override the computed priority |
| `cx cases clear-priority <id>` | Remove a priority override |
| `cx cases mute <id>` / `cx cases unmute <id>` | Stop / restore a case's notifications; status is unchanged |
| `cx cases set-parent <child-id> --parent <parent-id> --mute-child\|--no-mute-child` | Make the first case a child of `--parent` — see "Case relationships" |
| `cx cases unlink <id> --linked <other-id>` | Remove a parent-child link |
| `cx cases linked <id>` | Read a case's parent or children |
| `cx cases events list <case-id>` | Event timeline (status changes, comments, assignments) |
| `cx cases events list <case-id> --include-children-events` | Parent timeline merged with its children's events |
| `cx cases events get <event-id>` | A single event — drill in, e.g. to expand a comment thread |
| `cx cases notifications <case-id> [<case-id> ...]` | Notification deliveries (connector, status, time) |

## What the CLI cannot do

Tell the user plainly and point at the alternative — do not improvise a command
that does not exist.

| Not available | Why, and what to do instead |
|---|---|
| **Attaching indicators to a case** | A case created with `cx cases create` has no indicators, and none can be added later. If the issue should be tracked by an alert, fix or create the alert (`cx-alerts`) and let the Cases service open its own case. |
| **Listing / searching cases** | `cx cases list` was removed. Case discovery lives in the dataset: run `cx dataprime query "source system/labs.cases.state_updates \| ..." --start now-24h` (there is no `--from`; the window is `--start`/`--end`). See [`references/case-analytics.md`](references/case-analytics.md). |
| **"Related" or "duplicate" links** | Only parent-child links exist. To tie two cases together, make one the child of the other (`cx cases set-parent`). |
| **Changing labels** | Labels are fixed when the case is created. `cx cases update` patches only the title, resolution reason and description. |
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

## Creating a case

`cx cases create` opens a case on demand — for something you found while
investigating, a user report, or anything no alert covers.

```bash
cx cases create \
  --title "Payments API p99 latency degraded" \
  --priority P2 \
  --description "p99 latency on payments-api rose from ~250ms to ~900ms starting 2026-10-27 10:00 UTC; error rate unchanged. Started with deploy v4.12. Evidence: <query/dashboard links>. Impact: checkout slow for EU users." \
  --label service=payments-api --label routing.team=payments \
  --assignee you@example.com
```

A manual case is **fully managed by the user**:

- **No indicators.** It is not linked to any alert or Error Tracking issue. The Cases service never updates, auto-resolves, or reopens it — nothing
  will change its status but people. Whoever creates it owns driving it to
  `RESOLVED`/`CLOSED`.
- **The description is the only context.** There are no alert definitions,
  permutations, or triggering logs to pivot into, so anyone triaging it — a
  teammate or an agent — sees only what you wrote. `--description` is required by
  the CLI. Write it to stand on its own: what is wrong, observed impact, the time
  window (absolute UTC, not "an hour ago"), affected services/entities, the
  evidence (exact queries, numbers, dashboard or console links you actually saw),
  and what has been ruled out. Keep it current with `cx cases update --description`
  (see "Updating the description"); short follow-ups go in `cx cases comment`.
- **Labels** (`--label key=value`, repeatable) drive filtering, category
  derivation, and notification routing — `routing.*` labels pick the router.
  Unlike the description, they are fixed at creation.
- **Initial status** is `ACTIVE` unless `--status ACKNOWLEDGED`. Only start it
  acknowledged when a person is actually on it.
- **The readable `CASE-…` id comes a moment later.** The backend assigns it after
  the create returns; `cx cases create` re-reads the case for up to 5 seconds to
  report it. If it is still missing, refer to the case by its title and UUID —
  the printed console link opens it now — and say the readable id will appear
  shortly. Never compose a `CASE-…` id yourself.
- **The description is write-only.** The Cases UI shows it, but `cx cases get`
  does not return it. Don't re-fetch to check it, and don't report a case as having
  no description on that basis.

Before creating, ask the user to confirm title, priority, and description — a case
notifies people. Prefer one case per issue. Nothing deduplicates a manual case, so
check for an existing open case on the same problem first (see
[`references/case-analytics.md`](references/case-analytics.md)); the dataset lags by
seconds to minutes, so a case opened moments ago may not show yet. If one already
covers it, ask whether to comment on that case or create a child; if you create,
link yours as its **child** (`cx cases set-parent <new> --parent <existing>`) so
their case stays the incident of record.

### Updating the description

`cx cases update <id> --description "<text>"` **replaces the whole description**;
nothing is appended. Use it to keep a manual case current as the investigation moves
on: new findings, the confirmed root cause, the mitigation.

- Pass the complete new description, in the same shape as at creation, at most
  10,000 characters. Build it from the text you wrote; if you don't have the
  current text, ask the user for it or for the full replacement. Never pass only
  the new part — it erases the rest.
- Short notes, or anything that belongs in the history, go in `cx cases comment`.
  The description holds the current picture; the timeline holds how it got there.
- Show the user the full new description and confirm before running the update.

## Triage Workflow

1. **Inspect** — `cx cases get <id>`. The payload includes `groupings`,
   `labels`, `impactedEntities`, `kpiBreaches`, `aiSummary`, and both
   `priorityDetails.system` (computed) and `priorityDetails.override` (user-set).
2. **Investigate** — Pull the underlying telemetry by querying the alert's DataPrime / PromQL to find root cause before acting.
   For a manually created case (no indicators), the description is the starting
   point — reproduce the evidence it cites rather than trusting it blindly.
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
confident. For non-resolution edits (title, description, post-hoc postmortem
link), use `cx cases update`.

## Case relationships (parent / child)

Cases can be linked into a **one-level** parent-child hierarchy: the parent is the
incident of record and children hang under it. Every case is exactly one of
**standalone**, **parent** or **child**, never both a parent and a child. A case has at most one
parent, and a parent has at most 100 children,
so there are no grandparents and no cycles. There is no "related" or "duplicate"
link type.

A child is usually **muted**, so the parent becomes the single notifying case.
Expect a child to be quiet; that is the design, not a delivery failure.

`cx cases get <id>` returns the case's `relationship`: absent for a standalone case,
`{"type": "PARENT"}` for a parent, `{"type": "CHILD", "parentCaseId": "<uuid>"}` for
a child. A parent does not inline its children — to list them, see
[Case relationships](references/case-analytics.md#case-relationships-parent--child)
in the analytics reference.

`cx cases linked <id>` reads a case's parent or children directly.

When triaging a child, read the parent first — the investigation usually lives
there. When resolving a parent, check its children: they do not resolve with it.

### Linking and unlinking

**`cx cases set-parent <id> --parent <parent-id>` makes `<id>` the child.**
`cx cases set-parent CASE-390 --parent CASE-383` makes **CASE-390 the child** and
**CASE-383 the parent** — always this direction. To make CASE-383 the child instead,
swap them. Both accept a readable ID or a UUID.

Before linking:

- Read the current hierarchy with `cx cases linked` for both cases.
- Restate the result as a sentence — "CASE-390 becomes the child of CASE-383" — and
  have the user confirm that sentence.
- **Choose the mute explicitly**: `--mute-child` or `--no-mute-child` is required.
  `--mute-child` stops the child notifying so the parent is the incident of record;
  `--no-mute-child` leaves both notifying. If the child is notifying today, say so in
  the confirmation — "CASE-390 is notifying <team>'s on-call; linking it with
  `--mute-child` stops that." The user confirms that sentence, not just the link.

The backend enforces the shape, each with a clear error:

| Error | Meaning | Do |
|---|---|---|
| `Case is already a child of another case` | One parent maximum | Report the existing parent; offer to unlink first |
| `Target case is already a child…` | No grandparents | The target cannot be a parent; offer the top of the chain |
| `Cannot detach from parent: case is not a child` | Nothing to unlink | Run `cx cases linked` and report the real hierarchy |

A parent takes at most 100 children. Linking a resolved or closed case is allowed.
`cx cases unlink <id> --linked <other-id>` removes a link. If the child was linked
with `--mute-child` and should notify again, run `cx cases unmute` on it.

To read a parent's timeline together with its children's, run
`cx cases events list <parent-id> --include-children-events`. Every event carries
the `caseId` it belongs to (shown as a Case ID column in text output), so you can
tell parent events from child events. Child events are subject to the caller's
access checks.

## Where a case came from

A case has **no top-level source field**. Provenance lives on its indicators: alert
indicators for cases opened by an alert, and generic indicators (with an
`indicatorType` and a free-form `metadata` object) for non-alert sources. That also
means `cx cases` cannot filter by source — see
[Generic indicators](references/case-analytics.md#generic-indicators--non-alert-sources)
in the analytics reference for the query.

A generic indicator's `externalId` is the dedup key. One unresolved case exists per key, and
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
  Assign it, or `cx cases mute` it.
- **Prefer muting over resolving** when the work is real but nobody is on it. A
  resolved case is a claim the system is healthy. Muting changes no status, but it
  silences someone's on-call — confirm before muting.
- **Confirm before every change** — create, update, comment, assign, priority,
  resolve, close, mute, set-parent, unlink. Reads need no confirmation.
- **Don't retry a change you aren't sure failed** — a retried `comment` posts twice.
- **A case has no source field** — provenance is on its indicators. See
  "Where a case came from". A case with no indicators at all was created manually.
- **Manual cases are user-owned** — nothing closes them automatically, and their
  description is the only context; write it to stand on its own.
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
