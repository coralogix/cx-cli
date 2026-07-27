# Health conditions model

The shared contract every health reference follows, and how this skill turns "is my data healthy?"
into an actionable verdict.

## What a "health condition" is

A condition is a checkable statement about telemetry that an experience or extension needs in order to
deliver value. Three kinds:

1. **Presence / continuity** — the signal is arriving *now* (not just historically). "Spans for
   `checkout` in the last 15 min."
2. **Completeness** — the required fields/attributes are present. "Spans carry `service.name` and a
   duration; errors are flagged."
3. **Quality** — the data meets a usefulness bar. "Not 100%-sampled by accident; app/subsystem names
   aren't blank or defaulted; no high-cardinality explosion."

## The verdict

Each condition resolves to one of:

| Verdict | Definition |
|---|---|
| **healthy** | condition met |
| **degraded** | data present but a completeness/quality condition fails |
| **missing** | required signal absent |

An experience's overall verdict is the worst of its conditions. **Always report the failing condition
+ why it matters + the route to fix it** — a bare "degraded" is not actionable.

## The onboarding ↔ health loop

```
cx-onboarding  ──instrument──▶  data flows  ──▶  cx-system-health checks conditions
      ▲                                                     │
      └──────────── gap: missing/degraded ◀─────────────────┘
```

When a condition is `missing` or `degraded`, hand back to `cx-onboarding` for the specific signal
(e.g. degraded APM completeness → `onboarding-apm-spans.md` to set `service.name`). The **automatic**
version of "detect the gap, then instrument it" is discovery-gated activation; this
skill is the manual/assistive path.

## How to check a condition (read-only)

- **Presence/continuity:** query the signal for a recent window; optionally compare to a baseline
  window to detect a *stop* (was flowing, now isn't).
  ```bash
  cx spans "filter \$l.serviceName == '<svc>'" --start now-15m --limit 1     # now
  cx spans "filter \$l.serviceName == '<svc>'" --start now-24h --end now-1h --limit 1  # baseline
  ```
- **Completeness:** inspect a sample and check for required fields, or use field discovery.
  ```bash
  cx spans "filter \$l.serviceName == '<svc>'" --limit 5 -o json
  cx search-fields "<required attribute>" --dataset spans
  ```
- **Quality:** surface-specific (sampling rate, blank names, cardinality) — defined per reference.

## What backs the condition data

Today this skill derives verdicts from **read-only `cx` queries**. It's written to a stable
**conditions → verdict → remediation** contract, so if a product health API becomes available it can
back the same verdicts without changing the model — only the *source* of a condition's status changes,
not the model or the loop. Write references as "here is the condition and how to verify it."

## Cost & AI

Checks are read-only `cx` queries → no ingestion quota, no AI Units. AI layers (optional): Layer-2 can
rank/phrase verdicts with a cheap model; Layer-3 Olly can diagnose+propose fixes (credit-gated). Any
absorbed AI cost must be explicit in COGS.

## Sources / evidence

Coralogix system-health — verdict/condition framing ("monitor the monitor"). Read-only `cx` checks;
the loop to instrumentation is the discovery-gated activation path. Created 2026-07.
