# Health: APM / tracing (a service)

*Validated end-to-end (2026-07).*

Is a service's tracing data healthy and complete enough for APM (service map, latency, errors) to be
useful? Checks presence, completeness, and quality of spans for one service.

## When to use this reference

"Is my APM healthy", "why is the service map incomplete", "my traces look wrong", "did tracing stop
for `<service>`", "is `<service>` fully instrumented for APM".

## Conditions & checks

| # | Condition | Kind | Check | Fail → |
|---|---|---|---|---|
| 1 | Spans arriving now for the service | Presence | `cx spans "filter \$l.serviceName == '<svc>'" --start now-15m --limit 1` | **missing** → `cx-onboarding` `onboarding-apm-spans.md` |
| 2 | Was flowing, still flowing (no stop) | Continuity | compare now vs baseline `--start now-24h --end now-1h` | **degraded** (stopped) → check collector/exporter |
| 3 | `service.name` / `cx.subsystem.name` set (not blank/defaulted) | Completeness | `cx spans "filter \$l.serviceName == '<svc>'" --limit 5 -o json` → inspect | **degraded** → set resource attributes (onboarding APM) |
| 4 | Errors are flagged (status/`error` present) | Completeness | inspect sample for status code / error attribute | **degraded** → instrument error semantics |
| 5 | Duration present (latency computable) | Completeness | inspect sample for span duration | **degraded** → SDK/auto-instrumentation gap |
| 6 | Sampling is intentional (not accidental 100% or ~0%) | Quality | check span volume vs request volume / sampling config | **degraded** → set sampling on collector/exporter |
| 7 | Trace continuity (context propagated across services) | Quality | look for orphan/broken traces in Explore Spans | **degraded** → fix propagation |

## Verdict → remediation

- **missing** (cond. 1): the service isn't sending spans → route to `cx-onboarding` →
  `onboarding-apm-spans.md`. If the customer runs a legacy/proprietary APM agent, flag the bridge path
  and its sampling implications (see that reference).
- **degraded** (cond. 3–7): data flows but isn't fully useful → the specific fix, usually a small
  attribute/sampling/propagation change routed through onboarding.

## Quality gotchas specific to APM

- **Accidental 100% sampling** (e.g. a legacy agent bridged through OTel switched off adaptive
  sampling) inflates cost and can still look "healthy" on presence — check condition 6 explicitly.
- **Blank/`unknown_service`** means resource attributes weren't set — data lands but the service map is
  wrong. Common and high-impact.

## Tier note

APM/tracing requires a tier that includes it — on Low/Block the *experience* may be unavailable even
when spans arrive. A "healthy data / no visible experience" case is a **tier** verdict, not a data
verdict; say so and don't send the user chasing instrumentation.

## AI layers

- **Layer 1 (no-AI):** the condition table above.
- **Layer 2:** rank which degraded condition to fix first for this service.
- **Layer 3 (Olly, paid):** "diagnose why APM for `<service>` is degraded and propose the fix."

## Docs deep-links

- [Explore spans](https://coralogix.com/docs/user-guides/data_exploration/spans/)
- [APM via OpenTelemetry on Kubernetes](https://coralogix.com/docs/opentelemetry/integrations/apm-kubernetes-open-telemetry-opentelemetry/)

## Sources / evidence

Coralogix APM/tracing health conditions (`service.name`, error/latency completeness, sampling). The
bridged-agent accidental-sampling-change is a known real-world failure mode. Read-only `cx spans`
checks. Created 2026-07.
