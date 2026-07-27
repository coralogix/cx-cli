# Health: Fleet / collector health

*Validated end-to-end (2026-07).*

Is the collector fleet healthy — are all expected collectors reporting, on the intended config/version,
and not restart-looping? This is the ongoing-health counterpart to `onboarding-fleet.md` (which *deploys*
a collector). Covers ongoing **Fleet Health**.

## When to use this reference

"Are my collectors healthy", "which agents stopped reporting", "is my fleet reporting", "collector
config drift", "did an agent go down", "fleet health", "are all my OTel collectors up", "is the gateway
running".

> **UI-first.** Fleet Health has a dedicated UI. This reference is the terminal companion —
> quick programmatic checks + "what's wrong, where to fix it". For visual triage point the user to the
> Fleet Health dashboard.

> **Fleet Health model.** Fleet Health classifies via out-of-the-box **policies** into
> **Critical / Warning / Healthy** across the canonical failure modes: **agent down** (Availability),
> **throughput drop / no data** (Freshness), **config rejected** and **version drift** (Other). This
> reference uses those same modes; its verdicts map: `missing`≈no fleet at all, `degraded`≈Critical/Warning
> policy. Predefined policies are **enable/disable-only** (not editable); customers **add their own** rules
> (Fleet-only). A broken policy can raise a **Case**.
>
> **Health API (future).** If Fleet Health exposes a health API, the skill can call it for the verdict.
> Today the checks below hand-run read-only `cx` queries — same conditions → verdict contract, no rework later.

## Conditions & checks

| # | Condition (failure mode) | Category | Check | Fail → |
|---|---|---|---|---|
| 1 | Collectors emitting their own telemetry at all | Availability | `cx metrics search --name '*otelcol*'` | no fleet → `cx-onboarding` `onboarding-fleet.md` |
| 2 | **Agent down** — each expected collector still reporting (none silently gone) | Availability | per-collector `otelcol_*` now vs baseline (`--start now-24h --end now-1h`) | Critical → check pod/host, redeploy |
| 3 | All expected **roles** present (DaemonSet agent · cluster collector · gateway) | Availability | confirm each role reports (a DaemonSet fleet can be healthy while the gateway is not) | Warning/Critical → fix the Helm release for the missing role |
| 4 | **Throughput drop** — data still flowing at expected volume (not nop-config) | Freshness | compare recent throughput vs baseline; "no data in 5 min" style | Warning/Critical → nop/rejected config or upstream stop |
| 5 | **Config rejected / restart loop** — collector accepted its config and is stable | Other | `otelcol_exporter_send_failed_*` / restart counts / collector logs | Critical → endpoint/key/resources; re-push valid config |
| 6 | **Version drift** — collectors on the intended config/agent version | Other | compare version each collector reports via OpAMP vs intended Fleet config | Warning → push intended config via Fleet Management (OpAMP) |

*(These are exactly the modes the Fleet Health experience surfaces per agent → cluster → fleet. When the
1.2 health API ships, call it instead of the queries above — same contract.)*

## Verdict → remediation

- **No fleet** (cond. 1): no collector telemetry → route to `cx-onboarding` → `onboarding-fleet.md` (deploy).
- **Critical / Warning:**
  - agent down (2) → redeploy / check the node/pod.
  - missing role (3) → fix the Helm values (receiver vs cluster collector vs gateway) — evaluate roles separately.
  - throughput drop (4) → nop/rejected config or an upstream stop; check the source + collector config.
  - config rejected / restart loop (5) → collector logs: wrong endpoint/region, bad key, or resources; re-push valid config.
  - version drift (6) → push the intended config via **Fleet Management (OpAMP)** — no redeploy needed.
- **Raise a Case** for a Critical verdict where the customer has Fleet Health policies, instead of
  ad-hoc alerting.

## Fleet-specific gotchas

- **Split health by role.** "The fleet is healthy" is per-role: the DaemonSet agent can be fine while the
  gateway is failing tail-sampling (or vice-versa). Always evaluate roles separately.
- **Config drift ≠ agent down.** A collector can be up and reporting yet running a stale config — that's a
  quality verdict (cond. 4), not a presence verdict. OpAMP is how it's corrected without redeploy.
- **All Helm customers stream config via OpAMP** — so Fleet Health generalizes across the fleet, not just
  a special cohort.

## Tier note

Fleet/collector health is tier-agnostic (it's about the transport). The *signals* the fleet carries
inherit their pillar's tier behaviour — see the relevant per-signal health reference.

## AI layers

- **Layer 1 (no-AI):** the condition table above.
- **Layer 2 (minimal free):** rank which unhealthy collector/role to fix first; summarize a collector
  error log into a likely cause (cheap model, low token).
- **Layer 3 (Olly, paid):** "diagnose why the fleet is degraded and propose/deploy the fix" (credit-gated;
  Olly API → consumes AI Units).

## Docs deep-links

- [Fleet Management for Kubernetes (OpAMP remote config)](https://coralogix.com/docs/user-guides/fleet-management/fleet-remote-config-kubernetes/)
- [Integration troubleshooting (collector)](https://coralogix.com/docs/opentelemetry/kubernetes-observability/troubleshooting/)

## Sources / evidence

Coralogix Fleet Health — out-of-the-box policies, per-role split (DaemonSet / cluster collector /
gateway), OpAMP config across all Helm collectors. Read-only `cx metrics` checks. Created 2026-07.
