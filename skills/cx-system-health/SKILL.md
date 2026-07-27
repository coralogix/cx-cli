---
name: cx-system-health
description: |
  Use this skill to check whether telemetry ALREADY in Coralogix is healthy and complete enough for a
  given experience or extension to deliver value — coverage, continuity, and quality of the data, not
  querying it. Trigger when the user asks "is my data healthy", "what telemetry am I missing",
  "why is APM/Infra/RUM empty or incomplete", "my data used to flow and stopped", "did my logs stop",
  "is my instrumentation good enough", "check my observability coverage", "why isn't this dashboard
  populating", "does this extension have the data it needs", "is my service fully instrumented",
  "monitor the monitor", "check the health of my collectors' data", "are my collectors healthy",
  "which agents stopped reporting", "collector config drift", "fleet health", or wants to know whether
  an experience/extension has the signals and attributes it requires. When a gap is found, this skill
  routes to `cx-onboarding` to instrument the missing piece.
metadata:
  version: "0.1.0"
---

# Coralogix System Health Skill

Use this skill to answer **"is my telemetry healthy and complete enough to get value?"** — the
coverage/quality/continuity side of observability, distinct from *getting data in* (`cx-onboarding`)
and *investigating* data (`cx-telemetry-querying`). It checks whether each **experience** (APM, Infra
Explorer, RUM, Fleet Health…) and **extension** (quick-start integration packages) has the signals and
attributes it needs, reports a **verdict**, and when something is missing, hands back to
`cx-onboarding` to instrument it.

> **UI-first, CLI-companion.** Troubleshooting and system health are *primarily a UI experience* in
> Coralogix (the health dashboards). This skill is the **programmatic companion** for users already in
> the terminal: it verifies conditions, tells them exactly what's missing, and routes them to the fix.
> It does **not** replace the UI dashboard — for visual triage, point the user there.

---

## The core idea: health conditions → verdict → remediation

Every experience/extension has **conditions** its telemetry must meet to be useful (e.g. APM needs
spans flowing *with* `service.name` set *and* error/latency populated). This skill:

1. **Picks the experience/extension** the user cares about (or scans several).
2. **Loads the matching reference** from the table below — each defines that surface's conditions.
3. **Checks each condition** with read-only `cx` queries.
4. **Emits a verdict per condition:** `healthy` · `degraded` · `missing`.
5. **On `missing`/`degraded`, routes back to `cx-onboarding`** for the specific signal to instrument
   the gap (the onboarding ↔ health loop; the discovery-gated activation path is the automatic version).

> **How verdicts are computed.** Today this skill derives verdicts from **read-only `cx` queries**
> (see `references/health-conditions-model.md`). It's written to a stable *conditions → verdict →
> remediation* contract, so if a product health API becomes available later, the skill can call it
> without changing the model.

## Loading references

| The user wants to check… | Load this reference | Status |
|---|---|---|
| The health-condition model itself (verdicts, the loop, how to check) | `references/health-conditions-model.md` | **Full** |
| Fleet / collector health — agents reporting, roles present, config drift | `references/health-fleet.md` | **Full** |
| APM / a service's tracing health & completeness | `references/health-apm.md` | **Full** |
| Infra Explorer / metrics coverage health | `references/health-infra.md` | **Full** |
| An **extension**'s data requirements (does the extension have what it needs) | `references/health-extensions.md` | Stub — PM to complete |
| Logs / RUM / other surfaces | *(add per the template)* | Not yet authored |

Author new references from `contributing/health-reference-md-template.md`.

---

## Verdict model

| Verdict | Meaning | Action |
|---|---|---|
| **healthy** | All conditions met; the experience/extension can deliver value | Nothing to do |
| **degraded** | Data flows but a quality/completeness condition fails (e.g. spans without `service.name`, missing attributes, gaps in continuity) | Fix the specific attribute/config → often a small `cx-onboarding` change |
| **missing** | A required signal is absent entirely | Route to `cx-onboarding` for that signal |

Always report *which condition* failed and *why it matters* for that experience — a verdict without a
reason isn't actionable.

---

## How to check (read-only)

Health checks use read-only `cx` queries (no ingestion quota, no AI Units):

```bash
# Continuity: is the signal still arriving? (compare recent vs a baseline window)
cx logs "filter \$l.applicationname == '<app>'" --start now-15m --limit 1
cx spans "filter \$l.serviceName == '<service>'" --start now-15m --limit 1
cx metrics search --name '*<key-metric>*'

# Completeness: are the required attributes present?
cx spans "filter \$l.serviceName == '<service>'" --limit 5 -o json   # inspect for service.name, error, duration
cx search-fields "<attribute the experience needs>" --dataset spans

# Extensions/integrations present?
cx integrations list        # what's configured; cross-check against expected extensions
```

Each reference spells out the exact conditions and queries for its surface.

---

## AI usage & no-AI path (required posture)

Runs in the customer's own agent → **no Coralogix AI Units consumed** unless a step calls the Olly API.

- **Layer 1 — no-AI:** the deterministic condition checks + verdicts in the references. Always works
  (air-gapped/BYOC/out-of-units).
- **Layer 2 — minimal free AI:** optional — phrase a verdict in context, or rank which missing
  condition to fix first (low token, cheap model; absorbed cost must be explicit in COGS).
- **Layer 3 — full Olly (paid, credit-gated):** optional — "ask Olly to diagnose why this experience
  is degraded and propose a fix" (Olly API → consumes AI Units; fall back to Layer 1 when exhausted).

---

## Safety

Health **checks** are read-only and safe to run freely. **Remediation** (instrumenting a gap) is
mutating and is handled by `cx-onboarding` — apply its safety rules there (state what changes, dry-run,
user's own repo/cluster).

---

## Key principles

- **Organize by intent, not by our roadmap** — this skill is "is my data good enough"; onboarding is
  "get data in". Different intents, cross-linked in a loop.
- **A verdict needs a reason and a route** — say which condition failed, why it matters, and where to fix it.
- **Checks are read-only; fixes go through `cx-onboarding`.**
- **UI is the primary health surface** — this is the terminal companion, not a replacement.
- **Conditions are contributable** — each experience/extension owns its reference MD.

---

## Additional resources

### Reference files

- **`references/health-conditions-model.md`** — the verdict model, the onboarding↔health loop, how to check conditions.
- **`references/health-fleet.md`** — fleet / collector health (agents reporting, roles, config drift).
- **`references/health-apm.md`** — APM / tracing health conditions for a service.
- **`references/health-infra.md`** — Infra Explorer / metrics coverage conditions.
- **`references/health-extensions.md`** — extension data-requirement checks (stub).

### Authoring standard

- **`contributing/health-reference-md-template.md`** — template for a per-experience/extension health reference.

---

## Related skills

| User intent | Route to |
|---|---|
| Fix / instrument a missing or degraded signal (the remediation half of the loop) | `cx-onboarding` |
| Investigate a specific incident/error in data that flows | `cx-telemetry-querying` |
| Build a dashboard or alert on the health signals | `cx-dashboards` |
| Look up product docs | `coralogix-docs` |
