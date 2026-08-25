# Onboarding: APM / Traces (spans)

Get distributed traces from a service into Coralogix APM via OpenTelemetry. "Onboarded" means:
spans for the service are queryable and the service shows up in APM / Explore Spans with latency and
error data.

## When to use this reference

The orchestrator loads this file when the user wants to "send traces to Coralogix", "set up APM",
"instrument my service for tracing", "see spans / a service map", or "debug request latency by trace".

If spans already flow and the user wants to *analyze* them, route to `cx-telemetry-querying`.

## Prerequisites (in order)

1. **A destination for spans.** Either the **collector** (`onboarding-fleet.md`, recommended in
   Kubernetes — the gateway does tail sampling) or direct OTLP export from the app to
   `ingress.<coralogix-domain>:443`.
2. **Required format & params — OTLP protobuf over gRPC.** APM ingests **OpenTelemetry traces**;
   the exporter must emit OTLP protobuf, not a custom format. Required:
   - Endpoint: `ingress.<coralogix-domain>:443` (region from the `cx` profile), or the in-cluster
     collector endpoint.
   - Auth (direct export): `Authorization: Bearer <send-your-data-api-key>`.
   - Resource attributes: **`cx.application.name`** and **`cx.subsystem.name`** — these decide where
     the service appears. Also set `service.name` (OTel standard) for the service map.
3. **An instrumentation method** for the language/runtime: OTel **auto-instrumentation** (agent/init
   container, zero-code for supported frameworks) or the **OTel SDK** in code.

> ### ⚠️ Non-native / legacy APM agents don't export OTLP by default
> A vendor's proprietary APM agent (e.g. a .NET CLR profiler using bytecode injection) generally
> **cannot emit OTLP natively**. Bridging it (vendor agent → vendor OTel collector → OTel collector →
> Coralogix) is a real, supported-but-heavy path — and bridging can silently change behaviour (e.g.
> switching adaptive sampling to 100% raw capture). If the user is on a legacy agent, flag the bridge
> path and its sampling implications up front rather than discovering them months later.

## Minimal config (happy path)

**A. Kubernetes auto-instrumentation (zero-code)** — with the collector already running, annotate the
workload so the OpenTelemetry Operator injects auto-instrumentation, or use the language auto-agent.
See the APM-on-Kubernetes doc linked below for the per-language annotation.

**B. SDK / env-var export (any environment)** — point the app's OTel exporter at the endpoint:

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT="https://ingress.<coralogix-domain>:443"
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer <send-your-data-api-key>"
export OTEL_RESOURCE_ATTRIBUTES="cx.application.name=<app>,cx.subsystem.name=<service>,service.name=<service>"
# then run the app under the language's OTel auto-instrumentation, e.g. Python:
opentelemetry-instrument python app.py
```

(When exporting to the in-cluster collector instead of directly, set the endpoint to the collector
service and drop the auth header — the collector holds the key.)

## Verify (close the loop)

```bash
cx spans "filter \$l.serviceName == '<service>'" --start now-15m --limit 5
```

Expected: recent spans for the service, with duration and status. Then confirm it appears in APM /
Explore Spans in the UI. If empty after ~5–10 min, see Common failures.

## Common failures → fixes

| Symptom | Likely cause | Fix |
|---|---|---|
| No spans after 10 min | Exporter not OTLP protobuf, or wrong endpoint region | Confirm OTLP exporter; match endpoint to profile region |
| Spans land under wrong/blank service | `cx.subsystem.name` / `service.name` unset | Set resource attributes explicitly |
| Traces incomplete / broken | Context not propagated across services, or tail-sampling drops | Ensure propagation; review gateway sampling |
| Legacy .NET/Java agent won't export | Proprietary agent can't emit OTLP | Use OTel auto-instrumentation, or the documented bridge (mind sampling change) |
| 100% of spans captured, cost spikes | Bridge/agent switched off adaptive sampling | Configure sampling explicitly on the collector/exporter |

## Tier & cost

- **Tier interaction:** spans/APM require a tier that includes tracing; **Low/Block tiers may not
  surface APM features** — don't guide a user to a tier that hides the result. Confirm before routing.
- **Customer cost:** span volume × sampling rate drives egress + ingestion. Tail sampling on the
  gateway is the main lever.
- **Coralogix COGS:** span ingestion; no AI tokens in the Layer-1 path.

## AI layers for this signal

- **Layer 1 (no-AI):** the steps + `cx spans` verify above.
- **Layer 2 (minimal free AI):** optional — given a detected framework, suggest the right
  auto-instrumentation snippet (low token, cheap model).
- **Layer 3 (full Olly, paid):** optional — "ask Olly why traces are missing / why latency is high"
  (credit-gated; uses the Olly API → consumes AI Units).

## Docs deep-links

- [APM using OpenTelemetry as a unified shipper with Kubernetes](https://coralogix.com/docs/opentelemetry/integrations/apm-kubernetes-open-telemetry-opentelemetry/)
- [OpenTelemetry custom traces](https://coralogix.com/docs/integrations/data-ingestion/opentelemetry-custom-traces/)
- [Explore spans (querying, once flowing)](https://coralogix.com/docs/user-guides/data_exploration/spans/)
- [Coralogix endpoints (region → domain)](https://coralogix.com/docs/integrations/coralogix-endpoints/)

## Sources / evidence

Coralogix OTLP custom-traces + APM-on-Kubernetes docs; endpoints + Send-Your-Data key docs (verified
2026-07). The legacy-agent bridge + accidental sampling-change is a known real-world onboarding
failure mode.
