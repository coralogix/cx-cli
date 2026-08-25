# Health: Infra Explorer / metrics coverage

*Validated end-to-end (2026-07).*

Is infrastructure metric coverage healthy and complete enough for Infra Explorer to populate entities
(nodes, pods, hosts) with live values?

## When to use this reference

"Is my infra monitoring healthy", "why is Infra Explorer empty/partial", "did metrics stop", "are my
nodes/pods showing up", "is my metrics coverage complete".

## Conditions & checks

| # | Condition | Kind | Check | Fail → |
|---|---|---|---|---|
| 1 | Core infra metrics arriving now | Presence | `cx metrics query 'container_cpu_usage_seconds_total'` / `'kube_pod_info'` (instant — non-empty = live *now*; `metrics search` reads the untimed catalog and can't prove liveness) | **missing** → `cx-onboarding` `onboarding-metrics-infra.md` |
| 2 | Still flowing (no stop vs baseline) | Continuity | `cx metrics query 'rate(<key_metric>[5m])'` recent vs earlier | **degraded** (stopped) → check collector scrape |
| 3 | Entity-mapping labels present (node/pod/host identifiers) | Completeness | inspect labels on infra metrics | **degraded** → ensure infra semantic conventions |
| 4 | Expected metric names match dashboard/experience needs | Completeness | `cx metrics search --name '*<expected>*'` | **degraded** → naming mismatch (see below) |
| 5 | Cardinality sane (no label explosion) | Quality | check series count for high-cardinality labels | **degraded** → drop/aggregate labels |
| 6 | Scrape interval intentional (cost vs resolution) | Quality | review scrape config | **degraded** → tune interval |

## Verdict → remediation

- **missing** (cond. 1): infra metrics not collected → route to `cx-onboarding` →
  `onboarding-metrics-infra.md` (deploy/enable the collector's infra metrics).
- **degraded** (cond. 3–6): data flows but Infra Explorer is partial or costly → the specific fix
  (labels, naming, cardinality, interval).

## The naming-fragmentation trap (why "healthy metrics" ≠ "populated experience")

Metrics can be present and continuous yet the **default dashboards / Infra Explorer stay empty** because
the metric names don't match what the experience expects (8–10 naming variations are common — the hard
part of metrics activation). Condition 4 catches this: healthy *ingestion* but degraded *experience
coverage*. Report it as a naming/mapping gap, not a "no data" gap.

## Tier note

Metrics experiences (alerts/dashboards) vary by tier; on lower tiers some are unavailable even with
healthy data. Distinguish a **tier** verdict from a **data** verdict.

## AI layers

- **Layer 1 (no-AI):** the condition table.
- **Layer 2:** map raw metric names to the expected convention; suggest the label/interval fix.
- **Layer 3 (Olly, paid):** "diagnose why Infra Explorer is empty and propose the fix."

## Docs deep-links

- [Explore metrics](https://coralogix.com/docs/user-guides/data_exploration/metrics-explorer/)
- [Optimize metrics cost by scrape interval](https://coralogix.com/docs/user-guides/account-management/payment-and-billing/optimize-metrics-costs-in-coralogix-by-adjusting-your-scrape-interval/)

## Sources / evidence

Coralogix Infra Explorer / metrics health. Metric-name fragmentation is a well-known reason default
dashboards / Infra Explorer stay empty even when metrics flow. Read-only `cx metrics` checks. Created 2026-07.
