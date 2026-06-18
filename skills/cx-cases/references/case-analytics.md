---
name: case-analytics
description: Fleet/aggregate analytics across many cases via the `system/labs.cases.state_updates` DataPrime dataset. Load when the user asks counts, trends, distributions or rates across cases — e.g. "MTTR last week", "how many P1 cases today", "active case count per team", "list active cases", "how long until activation on average". Do NOT load for one specific case ID - that's `single-case`. The `alerts` and `dataprime` skills should be loaded alongside this skill.
---

# Case Analytics Skill — Fleet-level DataPrime queries over cases

## Pair with the `dataprime` skill

This skill covers two things and only two things: (1) the schema of `system/labs.cases.state_updates`, and (2) the case-counting conventions (1h minimum window, dedupe per `caseId`).
Use the schema in this skill to know **what fields exist and what they mean**, and use `dataprime` to know **how to shape the query around them**.

## Dataset: `system/labs.cases.state_updates`
Data for this dataset is only available from June 8th, 2026 onward. If a query requests earlier dates, inform the user that data starts from this date and adjust the query window accordingly.
A single events dataset that captures every state-change emitted by the Cases service, plus periodic **heartbeat** events for cases that are still active but otherwise inactive (so an open case still shows up inside a query window even when nothing happened to it). All queries in this skill target this dataset.

### Schema (representative event)

```json
{
  "eventLabels": {},
  "eventMetadata": {
    "timestamp": 1737017985447000000,
    "cxEventId": "0523fa80-fef0-4a7c-8d04-15a449becce5",
    "severity": "Info",
    "priorityClass": "medium",
    "entityType": "cases"
  },
  "userData": {
    "caseId": "76c411be-ff4d-4fb1-a987-5fce042deaaf",
    "caseNumber": 1234,
    "schemaVersion": 1,
    "metadata": {
      "trigger": "caseClosed",
      "change": {
        "$type": "statusChanged",
        "previousStatus": "RESOLVED",
        "currentStatus": "CLOSED"
      }
    },
    "title": "Test Case",
    "description": "This is the description of the Test Case",
    "assignee": {
      "$type": "coralogixUser",
      "coralogixUser": { "id": "test-user-id-123" }
    },
    "status": "CLOSED",
    "priority": "P4",
    "priorityDetails": { "system": "P4", "override": null },
    "category": "AVAILABILITY",
    "createdAt": 1754817300000000000,
    "activatedAt": 1754817330000000000,
    "updatedAt": 1754904300000000000,
    "acknowledgedAt": 1754817390000000000,
    "acknowledgedBy": null,
    "assignedAt": null,
    "firstInteractedAt": null,
    "firstTriggerAt": 1754817240000000000,
    "resolvedAt": 1754903700000000000,
    "closedAt": 1754904300000000000,
    "closedBy": {
      "$type": "coralogixUser",
      "coralogixUser": { "userEmail": "test-user@coralogix.com" }
    },
    "resolutionDetails": {
      "resolutionType": "SYSTEM",
      "resolutionReason": "This was a false alert.",
      "resolvedAt": 1754903700000000000,
      "resolvedBy": { "$type": "system", "system": {} }
    },
    "kpiBreaches": {
      "breachedKpis": [
        {
          "id": "0e0a2a2b-4e6f-4f8e-9e2a-1b2c3d4e5f60",
          "createdAt": 1754817400000000000,
          "kpiType": "TIME_TO_ACKNOWLEDGE",
          "casePriority": "P4",
          "breachedAt": 1754817400000000000,
          "mitigatedAt": 1754817500000000000,
          "breachStatus": "MITIGATED"
        }
      ]
    },
    "indicators": {
      "alerts": [
        {
          "instanceId": "instance-id-1",
          "alertDefinitionId": "987e4567-e89b-12d3-a456-426614174111",
          "alertVersionId": "887e4567-e89b-12d3-a456-426614174112",
          "title": "Test CPU Alert",
          "alertType": "METRIC_THRESHOLD",
          "priority": "P4",
          "groupingType": "COMBINATION_ALERT",
          "groupings": { "service": "payment-api", "region": "us-west-2" },
          "permutations": {},
          "labels": { "metric": "cpu", "team": "platform" },
          "state": "TRIGGERED",
          "isNoData": false,
          "triggeredAt": 1754817240000000000,
          "resolvedAt": null,
          "alertQuery": {
            "queryString": "avg(node_cpu_seconds_total{mode=\"system\",service=\"payment-api\"}) > 0.8",
            "type": "ALERT_QUERY_PROMQL"
          }
        }
      ]
    },
    "groupings": {
      "service": ["payment-api"],
      "region":  ["us-west-2"]
    },
    "labels": {
      "metric":          ["cpu"],
      "team":            ["platform"],
      "routing.service": ["cases"]
    },
    "notificationEvidences": [
      { "type": "slack",         "evidence": { "url": "https://coralogix.slack.com/archives/C0123456789/p1754817400000200" } },
      { "type": "service_now",   "evidence": { "url": "https://coralogix.service-now.com/nav_to.do?uri=incident.do?sys_id=abc123" } },
      { "type": "generic_https" },
      { "type": "pagerduty" },
      { "type": "email" }
    ],
    "aiSummary": "CPU saturation on payment-api in us-west-2 cleared after autoscaler added two pods.",
    "impactedEntities": [
      { "kind": "apmService",  "name": "payment-api", "language": "go" },
      { "kind": "apmDatabase", "name": "orders_db",   "system": "postgresql", "source": "db.name" }
    ]
  }
}
```

### Field reference (partial)

The dataset is one row per *state-update event* for a case. Because heartbeats fire for active cases, the **same `caseId` appears many times** within any window. Every analytical query must collapse to one row per case via the dedup patterns in [Hard rules](#hard-rules-always-apply) below.

#### Identity

| Field | Type | Meaning |
|---|---|---|
| `caseId` | UUID string | **Primary key.** All `groupby` / `dedupeby` operations key on this. |
| `caseNumber` | integer | Readable number behind the `CASE-<n>` ID shown in the UI. Useful for human-facing listings. |

#### Event metadata (this specific state-update)

| Field | Meaning |
|---|---|
| `metadata.trigger` | What triggered this event. Drives which lifecycle timestamp got set on this row. Common values: `caseCreated`, `caseActivated`, `caseAcknowledged`, `caseAssigned`, `caseResolved`, `caseClosed`, `caseArchived`, plus a heartbeat trigger emitted periodically for still-active cases. |
| `metadata.change` | A discriminated union describing the **diff** introduced by this event. The `$type` field is the discriminator. The most common variant is `statusChanged` with `previousStatus` and `currentStatus`. Use this when you want "transitions out of X" rather than "current state == X". |

#### Status & priority

| Field | Domain | Notes |
|---|---|---|
| `status` | `PENDING_ACTIVATION` / `ACTIVE` / `ACKNOWLEDGED` / `RESOLVED` / `CLOSED` | Filter on this after dedup for "currently in X" queries. |
| `priority` | `P1` / `P2` / `P3` / `P4` / `P5` | **String**, not numeric. Use equality (`== 'P1'`). |
| `priorityDetails.system` | same as `priority` | The system-derived priority. |
| `priorityDetails.override` | same as `priority`, or `null` | If set, the user overrode the system priority. When reporting priority, the override wins; |
| `category` | `AVAILABILITY` or `SECURITY` | Filter dimension. |

#### Lifecycle timestamps

**All timestamps in `userData` are numeric nanoseconds since epoch.** Cast with `:timestamp` for arithmetic and time-unit conversion, e.g. `(resolvedAt:timestamp - createdAt:timestamp).toTimeUnit('m')`. Null when the corresponding lifecycle step hasn't happened.

| Field | Meaning |
|---|---|
| `createdAt` | Case opened. |
| `activatedAt` | Moved `PENDING_ACTIVATION → ACTIVE`. `null` if never activated. |
| `updatedAt` | Last state change (any kind). |
| `acknowledgedAt` / `acknowledgedBy` | First acknowledge timestamp + actor. |
| `assignedAt` | First assignment timestamp. |
| `firstInteractedAt` | First human interaction (comment, status change by a person). |
| `firstTriggerAt` | When the underlying alert first fired. May be *earlier* than `createdAt`. |
| `resolvedAt` | Case resolved |
| `closedAt` / `closedBy` | Case closed (post-resolution). |

#### Alert indicators

`indicators.alerts[]` — the alerts that opened or feed this case.

Per-element fields:

| Field | Meaning |
|---|---|
| `alertDefinitionId` / `alertVersionId` | Stable IDs. Useful for joining with alert-side data via the `alerts` skill / `get_alerts_object`. |
| `priority` | Alert priority at trigger (independent of case priority). |
| `groupingType` | `STANDARD` / `COMPOSITE_ALERT` / `COMBINATION_ALERT`. The latter two mean the indicator combines multiple sub-conditions. |
| `state` | `TRIGGERED` (still firing) / `RESOLVED` / `NO_DATA` (signal dropped, **not** a recovery) / `MUTED`. |
| `triggeredAt` / `resolvedAt` | Indicator-side timing (different from case-level resolution). |

#### Groupings & labels (routing / attribution)

Both `groupings` and `labels` are `{ key: string[] }`

- `groupings` — the dimensions used to group the case (e.g. `service`, `region`, `cluster`).
- `labels` — free-form labels. The `routing.*` prefix is the convention for routing config:
  - `routing.team` — owner team (use this for "MTTR per team" style queries).
  - `routing.environment` — env (`prod`, `staging`, …).
  - `routing.service` — service tag.

#### Notifications

`notificationEvidences[]` — per-channel record of where the case's notifications landed. The `type` field is the discriminator.

| `type` | `evidence` shape | Notes |
|---|---|---|
| `slack` | `{ url: "<workspace>.slack.com/archives/<channel>/p<ts>" }` | Verbatim permalink to thread / channel. |
| `service_now` | `{ url: "<instance>.service-now.com/nav_to.do?uri=incident.do?sys_id=<id>" }` | Incident URL. |
| `pagerduty` | may or may not have `url` | PagerDuty often records only the incident ID, not a deep URL. |
| `email` | usually no `url` | Surface recipient/distribution alias if present. |
| `generic_https` | usually no `url` | Custom HTTPS connector. |

#### Other fields

`aiSummary` is a pre-computed string. Treat it as one input among many — never the source of truth.
`$m.timestamp` is the **event timestamp** (when the state-update was emitted) and is what you order by. `createdAt` (or `$d.createdAt`) is the **case creation time**.

## Hard rules (ALWAYS apply)

These three rules are non-negotiable for every query you write against `system/labs.cases.state_updates`. They exist because the dataset emits a heartbeat for active cases, so the **same caseId appears many times** within any window. Counting raw events double-counts cases.

### Rule 1 — Always provide a time range, minimum 1 hour

Every query MUST be executed with an explicit time range passed via the `time_filters` parameter. **Never query the dataset without a time range.** There is no "ever" / "all of time" semantics here: the dataset has a TTL, the scan is unbounded if you omit the window, and the result will silently reflect whatever the engine happened to load — misleading at best, wrong at worst.

The minimum window is `1h`. If the user asks for a shorter window, expand it to `1h` and tell them that the dataset is built for minimum 1h time ranges. If the user asks an open-ended "ever" / "in total" question, pick a sensible default window (`last 7d` is the default starting point for most case-state questions — active listings, current counts, recent resolutions; `last 30d` for resolution-style questions when more history is needed) and tell the user the window you assumed so they can override.

### Rule 2 — Always collapse to one row per `caseId`

Before counting, listing, or aggregating, you MUST reduce the stream to one row per case so each case is counted at most once. Use one of these two equivalent patterns:

**Pattern A — `groupby` keeping the latest event per case:**

```dataprime
| groupby caseId aggregate
    max_by($m.timestamp, $d) as latest
```

After this, the latest event payload is in `latest` (e.g. `latest.status`, `latest.priority`, `latest.createdAt`). Use this when you need the *latest known state* and want to read several fields off it.

**Pattern B — `dedupeby` keeping the most recent row:**

```dataprime
| dedupeby caseId orderby $m.timestamp desc
```

After this, the full row stays at top level (`status`, `priority`, `createdAt`, …). Use this when downstream commands read top-level fields directly.

The two patterns are interchangeable; pick whichever keeps the rest of the pipeline shortest.

**Exceptions** — you may skip dedup only when you are *intentionally* counting events (not cases), e.g. "how many `caseResolved` triggers fired"; in that case `filter metadata.trigger == 'caseResolved' | distinct caseId | count` is still the correct shape because `distinct caseId` is the dedup.

### Rule 3 — When asking about a one-time lifecycle event, filter on `metadata.trigger`

Most questions about a single lifecycle moment (activation, resolution, closure, …) read better as a trigger filter than as dedup-then-filter. The trigger filter naturally excludes heartbeats *and* keeps the query simple:

```dataprime
| filter metadata.trigger == 'caseResolved'
```

This works because most lifecycle triggers fire **once per case** in the normal flow. Pick the right pattern from the table below.

| Trigger | Cardinality | Query |
|---|---|---|
| `caseCreated`, `caseActivated`, `caseResolved`, `caseClosed`, `caseArchived` | At most once | Filter alone is sufficient. |
| `caseAcknowledged` | **Can repeat** (re-acknowledge) | Filter fine for most; add `\| distinct caseId` to count cases, not events. |
| `caseAssigned` | **Can repeat** (reassignments) | Same — add `\| distinct caseId` to count cases, not events. |
| Heartbeat | Periodic | **Never** use as a lifecycle event; filter out or use Rule 2 dedup. |

**When in doubt, ask:** am I counting **events of a kind** or **cases in a state**? Events of a kind → Rule 3 (trigger filter). Cases in a state → Rule 2 (dedup to latest row).

## Query examples

### Time to activate

How long, on average, between case creation and activation (default window `last 7d` — adjust to the user's intent):

```dataprime
source system/labs.cases.state_updates
  | filter metadata.trigger == 'caseActivated'
  | create activation_lag from (activatedAt:timestamp - createdAt:timestamp).toTimeUnit('s')
  | aggregate avg(activation_lag) as avg_lag,
              percentile(0.5, activation_lag) as median_lag,
              max(activation_lag) as max_lag,
              count() as total_count
```

> `caseActivated` is emitted once per case, so no per-case dedup is needed here. If the user asks "ever" or "in total", expand the window deliberately (`last 30d`, `last 90d`) and tell them the window you picked — never omit it.

### Verifying resolutions

Number of distinct cases resolved in the last 30 days (counts each case at most once via `distinct`; pick an explicit window — never query "ever"):

```dataprime
source system/labs.cases.state_updates
  | filter metadata.trigger == 'caseResolved'
  | distinct caseId
  | count
```

Number of cases **currently** in `RESOLVED` (dedup by latest event per case):

```dataprime
source system/labs.cases.state_updates
  | groupby caseId aggregate
      max_by($m.timestamp, status) as currentStatus
  | filter currentStatus == 'RESOLVED'
  | count
```

### Case listing — last known state for every case

Default window `last 7d`, If the user is asking about long-running cases that may have been quiet for longer than that, widen explicitly (`last 14d`, `last 30d`) and tell them the window you used:

```dataprime
source system/labs.cases.state_updates
  | dedupeby caseId keep 1 orderby $m.timestamp desc
```

### Time-to-X bucketed over time (MTTR / MTTA)

Mean / median / p95 of a lifecycle duration, bucketed over time. Same shape for MTTR (resolve) and MTTA (acknowledge) — swap the trigger and timestamps.

```dataprime
source system/labs.cases.state_updates
  | filter metadata.trigger == 'caseResolved'
  | create ttr from (resolvedAt:timestamp - createdAt:timestamp).toTimeUnit('m')
  | create time_bucket from roundTime(resolvedAt:timestamp, 1d)
  | groupby time_bucket aggregate
      avg(ttr) as mttr,
      percentile(0.5, ttr) as median_ttr,
      percentile(0.95, ttr) as p95_ttr,
      count() as total_count
  | orderby time_bucket asc
```

> For MTTA, swap `metadata.trigger == 'caseAcknowledged'` and use `acknowledgedAt` in place of `resolvedAt`. For weekly buckets use `resolvedAt:timestamp / 7d` instead of `roundTime(...)`.

### MTBI daily

Mean time **between** incidents (i.e. between case creations), bucketed by day:

```dataprime
source system/labs.cases.state_updates
  | filter metadata.trigger == 'caseCreated'
  | create time_bucket from createdAt:timestamp / 1d
  | groupby time_bucket aggregate
      (max(createdAt:timestamp) - min(createdAt:timestamp)).toTimeUnit('m') as span_minutes,
      count() as total_count
  | create mtbi_minutes from span_minutes / (total_count - 1)
  | orderby time_bucket asc
```

> **Why this is exact, not an approximation.** The sum of consecutive gaps between events is a telescoping series: `(t2-t1) + (t3-t2) + … + (tn-t(n-1)) = tn - t1 = max - min`. So `avg gap = (max - min) / (count - 1)` matches what you'd get from averaging every individual gap. Edge case: `count == 1` yields zero gaps; guard with `if(total_count > 1, …, null)` if needed.

### Currently active cases

Count of cases currently in `ACTIVE`. Default window `last 7d`, widen if the user wants to include long-quiet cases:

```dataprime
source system/labs.cases.state_updates
  | groupby caseId aggregate
      max_by($m.timestamp, status) as currentStatus
  | filter currentStatus == 'ACTIVE'
  | count
```

List of currently active cases (one row per case via Pattern A):

```dataprime
source system/labs.cases.state_updates
  | groupby caseId aggregate
      max_by($m.timestamp, $d) as latest
  | filter latest.status == 'ACTIVE'
  | choose
      latest.caseId as caseId,
      latest.title as title,
      latest.priority as priority,
      latest.assignee as assignee,
      latest.createdAt as createdAt,
      latest.activatedAt as activatedAt
  | orderby createdAt desc
  | limit 1000
```

### MTTR / MTTA / MTBI per team

Per-team KPIs, dedup-then-explode pattern. The `labels['routing.team']` label drives team attribution; missing values bucket as `unassigned`. Default window `last 14d` — adjust to the user's intent:

```dataprime
source system/labs.cases.state_updates
  | dedupeby caseId orderby $m.timestamp desc
  | explode labels['routing.team'] into team original preserve
  | create team from if(team != null, team, 'unassigned')
  | create tta from (acknowledgedAt:timestamp - createdAt:timestamp).toTimeUnit('m')
  | create ttr from if(status == 'RESOLVED' || status == 'CLOSED',
      (resolvedAt:timestamp - createdAt:timestamp).toTimeUnit('m'),
      null)
  | create ts from createdAt:timestamp
  | groupby team aggregate
      avg(tta) as mtta,
      avg(ttr) as mttr,
      (max(ts) - min(ts)).toTimeUnit('m') as span_minutes,
      count() as total_count,
      count(if(status == 'RESOLVED' || status == 'CLOSED', 1, null)) as resolved_count,
      count(if(acknowledgedAt != null, 1, null)) as acknowledged_count
  | create mtbi from if(total_count > 1, span_minutes / (total_count - 1), null)
  | orderby mttr desc
```

### Alert discovery

List of alert definitions that opened cases recently. Explode the alerts array first, then dedup on the alert definition id:

```dataprime
source system/labs.cases.state_updates
  | dedupeby caseId orderby $m.timestamp desc
  | explode indicators.alerts into alert
  | choose alert.alertDefinitionId as id, alert.title as title
  | dedupeby id
```

## Best Practices
- See the query examples for default queries to base off for user questions.
- When building a query, **ALWAYS** follow the **Hard Rules**
  1. Time window ≥ `1h`.
  2. Dedup per `caseId` (Pattern A or Pattern B) before any `count` / `aggregate` that's meant to be "cases", not "events".
- Query planning:
  1. Source the dataset: `source system/labs.cases.state_updates` with a time
  2. Filter to the events you care about (e.g. `filter metadata.trigger == 'caseResolved'` for resolution analytics, or no trigger filter when you want the latest state of every case).
  3. Dedup per `caseId` if you're answering a "cases" question (Pattern A or B).
  4. Explode any array you need to fan out over (`indicators.alerts`, `kpiBreaches.breachedKpis`, `labels['routing.<key>']`, `impactedEntities`).
  5. Aggregate / project / order / limit.
