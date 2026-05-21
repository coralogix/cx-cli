---
name: cx-cases
description: >
  Use this skill when the user asks to "manage cases", "list cases", "triage case",
  "case investigation", "acknowledge case", "resolve case", "close case", "assign case",
  "unassign case", "open cases", "active cases", "case priority", "override case priority",
  "case status", "case timeline", "who is assigned to this case", "find cases by service",
  "filter cases", "high priority cases", "P1 cases", "security cases", "availability cases",
  "unresolved cases", "what cases are open", "case workload", "case backlog",
  "case events", "case event history", "case comments", "who commented on case",
  "case notifications", "case notification deliveries", "was the case notification sent",
  "where was the case notified", "case slack delivery", "case pagerduty delivery",
  "assign case to <email>", "give case to <person>", "hand case over",
  "who owns this case", "find user by email", "team members",
  or wants to triage and manage Coralogix Cases (the alert-grouping incident container)
  using the `cx cases` CLI commands.
metadata:
  version: "0.1.0"
---

# Cases Management Skill

Use this skill to list, inspect, and drive Coralogix Cases through their lifecycle - from active through acknowledged, resolved, and closed. Cases group related alert events into a single investigation unit with priority, category, and assignee metadata.

## CLI Commands

| Command | Purpose | Key flags |
|---|---|---|
| `cx cases list` | List cases with filters and pagination | `--status`, `--priority`, `--category`, `--assignee`, `--text`, `--page-size`, `--page-token` |
| `cx cases get <id>` | Get a single case by ID | - |
| `cx cases update <id>` | Update mutable fields | `--title`, `--resolution-reason` |
| `cx cases assign <id> --user <email>` | Assign a case to a user | `--user` (required; email preferred, raw user ID also accepted) |
| `cx cases unassign <id>` | Remove the case assignee | - |
| `cx cases acknowledge <id>` | Mark a case as acknowledged | - |
| `cx cases unacknowledge <id>` | Remove the acknowledgment | - |
| `cx cases resolve <id>` | Resolve a case | `--reason` (optional) |
| `cx cases close <id>` | Close a case (terminal state) | - |
| `cx cases set-priority <id> --priority <P1..P5>` | Override the computed priority | `--priority` (required) |
| `cx cases clear-priority <id>` | Remove a priority override | - |
| `cx cases filter-values` | Show aggregated counts per filter (status, priority, category, etc.) | - |
| `cx cases grouping-keys` | List grouping keys available for filtering | - |
| `cx cases events list <case-id>` | List the event timeline for a case (status changes, comments, assignments) | - |
| `cx cases events get <event-id>` | Get a single case event by its event ID | - |
| `cx cases notifications <case-id> [<case-id> ...]` | List notification deliveries for one or more cases (connector, status, delivery time) | - |

**Filter values:**
- `--status`: `PENDING_ACTIVATION`, `ACTIVE`, `ACKNOWLEDGED`, `RESOLVED`, `CLOSED` (also accepts full `CASE_STATUS_*` enums)
- `--priority`: `P1`, `P2`, `P3`, `P4`, `P5` (or `CASE_PRIORITY_P1` etc.)
- `--category`: `AVAILABILITY`, `SECURITY` (or `CASE_CATEGORY_AVAILABILITY` etc.)
- `--assignee`: a user ID, or the literal `unassigned` to show cases without an assignee
- `--text`: free-text search applied to case titles
- All list filters except `--text` are repeatable to OR-combine values (e.g. `--priority P1 --priority P2`)

**Output formats:** append `-o json` or `-o agents` to any read command (`list`, `get`, `filter-values`, `grouping-keys`). Write commands print a per-profile status line to stderr and the updated case to stdout in JSON/agents mode.

**Multi-profile:** use `-p <profile>` (repeatable) to fan out queries across multiple profiles simultaneously.

## Identifying People by Email

The Cases API internally uses opaque user IDs (UUIDs) for assignees, but **this CLI hides that detail from you**. You should always work with email addresses; the CLI resolves them via the team-members directory (`GET /api/v1/user/team/teammates`) under the hood.

Where this happens:

- **`cx cases assign --user <value>`** — when `<value>` contains `@`, it's treated as an email and resolved to the matching user ID before the assignment call. Raw user IDs are still accepted (useful in scripts where the ID is already known).
- **`cx cases list --assignee <value>`** — same rule: emails are resolved to user IDs before the filter is applied; `unassigned` is a reserved keyword for the "no assignee" filter.
- **All output formats (text, JSON, agents)** — assignees in case payloads are surfaced as the email address, not the user ID. If a user no longer exists in the team directory, the raw ID is shown as a fallback so you don't lose information silently.

**Rule of thumb when talking to the user:** never present a user ID in your answer. If you only have a user ID (e.g. from a stale case record), run `cx cases get <case-id>` again — the CLI will resolve it to an email — or tell the user the user is no longer on the team.

If `cx cases assign --user alice@example.com` errors with `no team member found`, the email is likely mistyped or the person isn't on this team. Check spelling against the live directory before retrying.

## Case Lifecycle

A case moves through these states, but transitions are **restricted** — there is no general "reopen" flow. Once a case is `RESOLVED` it can only progress to `CLOSED`, and `CLOSED` is terminal.

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
| `ACTIVE` | → `ACKNOWLEDGED`, `RESOLVED`, `CLOSED` | Ack is optional — you can resolve or close directly |
| `ACKNOWLEDGED` | → `ACTIVE`, `RESOLVED`, `CLOSED` | The only state that can go *back* to `ACTIVE` (via `cx cases unacknowledge`) |
| `RESOLVED` | → `CLOSED` only | **Not reopenable.** Cannot return to `ACTIVE` or `ACKNOWLEDGED` |
| `CLOSED` | (none) | **Terminal.** No further state changes accepted |

| State | Meaning |
|---|---|
| `PENDING_ACTIVATION` | Created but not yet visible/active |
| `ACTIVE` | Open, needs attention |
| `ACKNOWLEDGED` | A user has acknowledged ownership |
| `RESOLVED` | Marked as fixed; still open for follow-up but **cannot be reopened** |
| `CLOSED` | Final state, no further changes |

**Practical implications for an agent:**
- If you `resolve` a case prematurely, you cannot undo it — the only forward path is `close`. Confirm with the user before resolving when the situation is ambiguous.
- `unacknowledge` only works while the case is in `ACKNOWLEDGED` — it's the single supported "back" transition.
- For a false-alarm case you intend to discard, prefer `close` directly from `ACTIVE` rather than resolving first; that keeps the audit trail honest about whether real fix work was performed.

Categories: `AVAILABILITY` (reliability/outage) or `SECURITY` (security incident). Priorities: `P1` (highest) → `P5` (lowest).

## Triage Workflow

### Step 1: Survey the open caseload

```bash
# All active P1/P2 cases (the immediate work):
cx cases list --status ACTIVE --priority P1 --priority P2 -o json

# Get a high-level breakdown of every open case by status/priority/category:
cx cases filter-values -o json
```

### Step 2: Inspect a specific case

```bash
cx cases get <case-id>
cx cases get <case-id> -o json | jq '.case | {title, status, priority, assignee, impactedEntities}'
```

The case payload includes `groupings` (service/subsystem), `labels` (user-defined tags), `impactedEntities`, `kpiBreaches`, and `aiSummary` when populated.

### Step 3: Claim ownership

```bash
# Pass an email - the CLI maps it to the API's user ID for you:
cx cases assign <case-id> --user you@example.com
cx cases acknowledge <case-id>
```

Acknowledgment signals you're actively working on the case and stops re-notifications. Email is the preferred identifier — raw user IDs work too if you already have one in hand.

### Step 4: Drive to resolution

```bash
# Marking as resolved (one-way move - resolved cases cannot be reopened):
cx cases resolve <case-id> --reason "Rolled back deploy abc123 - root cause was a missing migration"

# Final close (terminal - no further changes accepted):
cx cases close <case-id>
```

**`cx cases resolve` is the one lifecycle command that requires explicit confirmation,** because resolution is irreversible. The CLI behaves like this:

- **Interactive terminal, no `--reason`:** prompts for the reason (required, non-empty), then prompts for confirmation. This is the default "manual" flow.
- **Interactive terminal, with `--reason`:** skips the reason prompt, still prompts for confirmation.
- **`--yes` flag:** skips both prompts. Use this in scripts/CI when the operator has already approved out-of-band. The reason becomes optional in this mode, but **omitting it is strongly discouraged** — empty resolutions destroy the audit trail.
- **Agent mode (Claude Code, Cursor, etc.):** the agent cannot answer the prompts. You must either supply `--reason` and `--yes` explicitly, or hand the command back to the user to run interactively.

**Always pass `--reason` when resolving.** The reason is visible to every teammate who later opens this case (and shows up in the event timeline), so it's the primary way you communicate *what actually happened* to people who weren't on the call. A good reason answers: what was the root cause, what fixed it, and what (if any) follow-up is needed. Treat it like a one-line postmortem rather than a status update.

Examples of useful vs. useless reasons:

| ❌ Low-signal | ✅ High-signal |
|---|---|
| `"Fixed"` | `"Restarted the payment-api pods - GC pause from leaked subscriber. Tracking longer-term fix in PROJ-1234."` |
| `"Resolved"` | `"False positive - alert threshold too aggressive after traffic shift. Tuned in PR #4567."` |
| `"Closed"` | `"Capacity issue on shard-3 - scaled up to 8 nodes. Capacity plan needs revisit (TODO: ping @sre)."` |

**Resolving is one-way.** A `RESOLVED` case can only transition forward to `CLOSED` — it cannot be moved back to `ACTIVE` or `ACKNOWLEDGED`. If the situation is uncertain, prefer staying in `ACKNOWLEDGED` (which *can* be reverted via `unacknowledge`) until you're confident.

If the case is a false alarm, `close` it directly from `ACTIVE` (skipping `resolve`) so the audit trail reflects that no fix work happened.

For non-resolution edits (e.g. correcting the title, adding a postmortem link after the fact), use `update`:

```bash
cx cases update <case-id> --resolution-reason "Updated post-incident: see https://wiki/postmortem/2026-05-22"
```

### Step 5: Adjust priority if needed

```bash
# Bump priority because impact is larger than the system computed:
cx cases set-priority <case-id> --priority P1

# Remove the override and let the system value stand:
cx cases clear-priority <case-id>
```

The case payload exposes both `priorityDetails.system` (computed) and `priorityDetails.override` (user-set) so you can tell which is currently active.

## Filter Recipes

### Find unassigned active cases

```bash
cx cases list --status ACTIVE --assignee unassigned -o json
```

### Cases assigned to a specific user

```bash
# By email (preferred):
cx cases list --assignee alice@example.com -o json | jq '[.[] | {id, title, status, priority}]'

# Raw user ID also works:
cx cases list --assignee <user-id> -o json
```

### Security cases needing triage

```bash
cx cases list --category SECURITY --status ACTIVE -o json
cx cases list --category SECURITY --status ACTIVE --status ACKNOWLEDGED -o json
```

### Text search by title

```bash
cx cases list --text "checkout" -o json
```

### Page through a large result set

```bash
cx cases list --page-size 100 -o json
# When more results remain, the CLI prints the next token to stderr; pass it:
cx cases list --page-size 100 --page-token "<token>" -o json
```

## Case Event Timeline

Use `events list` to see the full audit trail for a case — status changes, priority overrides, assignment changes, comments, and external sync activity. The event payload exposes the actor (user, system, or integration) and a timestamp:

```bash
cx cases events list <case-id> -o json
cx cases events list <case-id> -o json | jq '[.[] | {id, type, createTime}]'

# Drill into a specific event (e.g. expand a comment thread):
cx cases events get <event-id> -o json
```

Common event types include `EVENT_TYPE_COMMENT`, `EVENT_TYPE_STATUS_CHANGE`, `EVENT_TYPE_ASSIGNEE_CHANGE`, and `EVENT_TYPE_PRIORITY_OVERRIDE`. Use the timeline when you need to answer "who did what, when" during incident triage.

## Notification Delivery Audit

When a stakeholder asks "did the on-call actually get paged for this case?", use `notifications` to inspect what was sent and where. The endpoint accepts one or many case IDs and returns deliveries keyed by case ID:

```bash
# Single case:
cx cases notifications <case-id> -o json

# A batch of related cases (e.g. all P1s opened today):
cx cases list --status ACTIVE --priority P1 -o json \
  | jq -r '.[].id' \
  | xargs cx cases notifications -o json
```

The flattened rows include the destination connector (Slack/PagerDuty/email/etc.), delivery status, and timestamp - so you can quickly spot a routing rule that silently dropped notifications. If notifications are missing entirely, pivot to the `cx-observability-setup` skill to debug connectors and routers.

## Bulk Operations (manual)

The CLI does not expose bulk endpoints yet. To act on many cases at once, list them with `-o json`, pipe the IDs through `jq`, and loop:

```bash
cx cases list --status ACTIVE --priority P3 -o json \
  | jq -r '.[].id' \
  | xargs -I {} cx cases acknowledge {}
```

## Key Principles

- **Always use emails for users, never IDs** - `cx cases assign --user`, `cx cases list --assignee`, and all output show email addresses. The user-ID layer is an internal implementation detail; do not surface UUIDs to the user
- **Resolving is irreversible** - a `RESOLVED` case can only progress to `CLOSED`; it cannot be reopened. If uncertain, stay in `ACKNOWLEDGED` (which *can* be reverted with `unacknowledge`) until the situation is confirmed
- **`CLOSED` is terminal** - no transitions out, including back to `ACTIVE`. Closing is the final action on a case
- **Acknowledge is optional** - `ACTIVE` can transition directly to `RESOLVED` or `CLOSED` without going through `ACKNOWLEDGED`. Acknowledgment is for signaling ownership, not a required gate
- **For false alarms, close directly from `ACTIVE`** - skipping `resolve` keeps the audit trail honest about whether real fix work was performed
- **Always pass `--reason` when resolving** - the reason is how you communicate root cause, fix, and follow-up to teammates who weren't on the call. Treat it as a one-line postmortem, not a status word. The CLI itself enforces this in interactive mode by prompting when `--reason` is missing, and also requires an explicit confirmation because the action is irreversible. If you're in agent mode, supply `--reason` and `--yes` together — otherwise hand the command to the user to run manually
- **Filter values are repeatable** - `--status A --status B` means "A OR B"; do not pass comma-separated values
- **Use `cx cases filter-values`** to understand the shape of the open caseload before drilling in
- **`P1`-style shorthand is accepted** anywhere a priority/status/category is expected - no need to spell out the full `CASE_PRIORITY_P1` enum
- **Multi-profile fan-out** with `-p <profile>` makes cross-environment triage trivial - useful when cases exist in both `prod` and `staging`

## Related Skills

- **`cx-alerts`** - inspect the alert definitions that produced the alert events grouped into a case
- **`cx-telemetry-querying`** - pivot from a case's impacted entities into logs/spans/metrics for root cause analysis
