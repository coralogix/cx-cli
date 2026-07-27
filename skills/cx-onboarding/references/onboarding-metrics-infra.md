# Onboarding: Metrics & Infra Explorer

Get infrastructure and custom metrics into Coralogix and light up **Infra Explorer**. "Onboarded"
means: metrics are queryable with PromQL and infrastructure entities (nodes, pods, hosts) appear in
Infra Explorer with live values.

## When to use this reference

The orchestrator loads this file when the user wants to "send metrics to Coralogix", "monitor
infrastructure", "see CPU / memory / pods", "set up Infra Explorer", "scrape Prometheus into
Coralogix", or "send custom application metrics".

If metrics already flow and the user wants to *query* them, route to `cx-telemetry-querying`.

## Prerequisites (in order)

1. **A metrics source + destination.** In Kubernetes the **collector** (`onboarding-fleet.md`)
   scrapes infra + Prometheus endpoints and ships them. For app metrics, the OTel SDK or a Prometheus
   remote-write source can send directly.
2. **Required format & params.** Two supported paths:
   - **OTLP metrics (protobuf/gRPC)** to `ingress.<coralogix-domain>:443` with
     `Authorization: Bearer <send-your-data-api-key>` — same endpoint pattern as other signals.
   - **Prometheus remote-write** for existing Prometheus/scrape setups.
   In both cases set **`cx.application.name`** / **`cx.subsystem.name`** so metrics are attributed.
3. **Know the metric names / labels** you expect — metrics are found by name + labels, so a naming
   convention matters (fragmented names are the hard part of metrics activation; agree them early).

## Minimal config (happy path)

**A. Kubernetes infra metrics** — the `otel-integration` chart's agent collects node/pod/container
metrics out of the box once the collector is running (`onboarding-fleet.md`). No per-app work needed
for infra coverage.

**B. Custom app metrics via OTLP** — export from the app's OTel metrics SDK:
```bash
export OTEL_EXPORTER_OTLP_ENDPOINT="https://ingress.<coralogix-domain>:443"
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer <send-your-data-api-key>"
export OTEL_RESOURCE_ATTRIBUTES="cx.application.name=<app>,cx.subsystem.name=<service>"
```
(Export to the in-cluster collector instead of directly when a collector is present — drop the auth
header, the collector holds the key.)

**C. Existing Prometheus** — point Prometheus remote-write at the Coralogix Prometheus endpoint per
the docs (reuse existing scrape config; no re-instrumentation).

> **Scrape interval is a cost lever.** Default 1s/10s scrapes multiply metric volume. Set the interval
> deliberately per metric group — it directly drives customer cost and Coralogix COGS.

## Verify (close the loop)

```bash
# Is the metric present?
cx metrics search --name '*<metric>*'
# Query it (PromQL)
cx metrics query 'rate(<metric>[5m])'
# Infra metrics arriving?
cx metrics search --name '*container_cpu*'
```

Expected: the metric appears in search and returns a series; infra metrics like `container_cpu_*` /
`kube_pod_*` show up, and nodes/pods populate Infra Explorer in the UI. If empty, see Common failures.

## Common failures → fixes

| Symptom | Likely cause | Fix |
|---|---|---|
| Metric not found in search | Not sent yet, wrong endpoint region, or name differs | Confirm exporter/scrape target; match endpoint region; check exact name |
| Metric exists but Infra Explorer empty | Missing resource/semantic labels for entity mapping | Ensure infra semantic conventions (node/pod labels) are set |
| Huge metric volume / cost spike | Scrape interval too tight or high-cardinality labels | Raise scrape interval; drop/aggregate high-cardinality labels |
| Default dashboards don't populate | Metric names don't match expected conventions | Align names to the dashboard's expected metrics (naming fragmentation) |

## Tier & cost

- **Tier interaction:** metrics behaviour varies by tier (alerts/dashboards limited on lower tiers) —
  don't guide a user to a tier that hides Infra Explorer or alerting.
- **Customer cost:** metric volume = series × scrape frequency; scrape interval + cardinality are the
  levers. See [optimizing metrics cost by scrape interval](https://coralogix.com/docs/user-guides/account-management/payment-and-billing/optimize-metrics-costs-in-coralogix-by-adjusting-your-scrape-interval/).
- **Coralogix COGS:** metric ingestion; no AI tokens in the Layer-1 path.

## AI layers for this signal

- **Layer 1 (no-AI):** the steps + `cx metrics` verify above.
- **Layer 2 (minimal free AI):** optional — suggest a PromQL query for an intent, or map a raw metric
  name to a default dashboard (low token, cheap model).
- **Layer 3 (full Olly, paid):** optional — "ask Olly what's driving this metric / why infra looks
  unhealthy" (credit-gated; Olly API → consumes AI Units).

## Docs deep-links

- [Kubernetes complete observability — basic configuration (infra metrics)](https://coralogix.com/docs/external/telemetry-shippers/otel-integration/k8s-helm/kubernetes-observability/kubernetes-complete-observability-basic-configuration/)
- [OpenTelemetry custom metrics](https://coralogix.com/docs/developer-portal/apis/data-ingestion/opentelemetry-custom-metrics/)
- [Metrics API](https://coralogix.com/docs/user-guides/data-query/metrics-api/)
- [Explore metrics (querying, once flowing)](https://coralogix.com/docs/user-guides/data_exploration/metrics-explorer/)
- [Coralogix endpoints (region → domain)](https://coralogix.com/docs/integrations/coralogix-endpoints/)

## Sources / evidence

Coralogix OTLP custom-metrics + K8s observability docs; scrape-interval cost doc; endpoints doc
(verified 2026-07). Metric-name fragmentation is a well-known reason default dashboards / Infra
Explorer stay empty even when metrics are flowing.
