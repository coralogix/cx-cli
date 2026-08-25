# Onboarding: Collector & Fleet Management

Stand up the **OpenTelemetry collector** that transports telemetry to Coralogix, and (in
Kubernetes) put it under **Fleet Management** so its config can be updated remotely via OpAMP.
"Onboarded" means: the collector is running, healthy, and its own telemetry is visible in
Coralogix — everything else (logs, spans, metrics) rides on this transport.

## When to use this reference

The orchestrator loads this file when the user wants to "install the Coralogix collector",
"deploy the OpenTelemetry collector", "connect Kubernetes to Coralogix", "set up Fleet Management",
"configure the OTel agent", or "manage collector config remotely".

For **SDK-only apps** that export OTLP straight from the process, a standalone collector is optional
— route to `onboarding-apm-spans.md` / `onboarding-metrics-infra.md` and treat this file as the
Kubernetes/host path.

## Prerequisites (in order)

1. **A `cx` profile** with the right region + Send-Your-Data API key.
   ```bash
   cx profiles list        # confirm a profile exists and note its region
   cx profiles add <name>  # if not — prompts for region + API key
   ```
2. **Deploy tooling for the target environment.** Kubernetes: `kubectl` + `helm` with cluster access.
   Hosts/VMs: shell access to install the collector binary.
3. **The ingress endpoint + auth.** Collector sends **OTLP protobuf over gRPC** to
   `ingress.<coralogix-domain>:443` with `Authorization: Bearer <send-your-data-api-key>`. Resolve
   `<coralogix-domain>` from the profile region — see the
   [Coralogix endpoints doc](https://coralogix.com/docs/integrations/coralogix-endpoints/).
4. **A cluster / host name** to tag the collector's data with, and the **application / subsystem**
   naming convention you'll use downstream.

## Minimal config (happy path) — Kubernetes via Helm

Coralogix ships the **`otel-integration`** Helm chart (`otel-coralogix-integration`), which deploys an
OTLP **receiver** (agent DaemonSet: metrics + logs, load-balances spans) and a **gateway** (tail
sampling for spans).

1. Store the Send-Your-Data key as a secret (don't inline it):
   ```bash
   kubectl create secret generic coralogix-keys \
     --from-literal=PRIVATE_KEY=<send-your-data-api-key> -n <namespace>
   ```
2. Minimal `values.yaml` override — the two required values are the **endpoint domain** and the
   **cluster name**:
   ```yaml
   global:
     domain: "<coralogix-domain>"        # e.g. resolved from your profile region
     clusterName: "<my-cluster>"
   ```
3. Install / upgrade (dry-run first to review what changes):
   ```bash
   helm repo add coralogix-charts-virtual https://cgx.jfrog.io/artifactory/coralogix-charts-virtual
   helm upgrade --install otel-coralogix-integration \
     coralogix-charts-virtual/otel-integration \
     -n <namespace> --create-namespace \
     -f values.yaml --dry-run     # remove --dry-run to apply
   ```

Advanced topologies (central tail-sampling cluster, the OpenTelemetry Operator via a
`OpenTelemetryCollector` CRD) are in the docs — link them rather than expanding here.

## Verify (close the loop)

The collector emits its **own** telemetry. Confirm the pods are up, then confirm data in Coralogix:

```bash
kubectl get pods -n <namespace> | grep -i coralogix   # receiver + gateway Running
# Collector metrics arriving now? (instant query — result >0 means collectors are reporting;
# `metrics search --name '*otelcol*'` only lists the untimed catalog, so use it just to find names)
cx metrics query 'count({__name__=~"otelcol_.*"})'
# Any signal tagged with the cluster arriving?
cx logs "filter \$l.applicationname == '<my-cluster>'" --start now-15m --limit 5
```

Expected: collector pods `Running`, and `otelcol_*` process metrics (or your cluster's logs)
appearing within a few minutes. If empty, see Common failures.

## Common failures → fixes

| Symptom | Likely cause | Fix |
|---|---|---|
| Pods `CrashLoopBackOff` | Bad/missing API key secret, or wrong `domain` | Check the secret; confirm `global.domain` matches the profile region |
| Pods `Running`, no data in Coralogix | Endpoint region ≠ where you're querying, or key lacks send permission | Match ingress region to the profile; verify the Send-Your-Data key |
| Spans arrive but sampling looks off | Tail-sampling on the gateway | Review gateway sampling policy in `values.yaml` |
| Config change needs a redeploy every time | Not yet under Fleet remote config | Enable Fleet Management (below) to push config via OpAMP |

## Fleet Management (remote config via OpAMP)

Once the collector is running, **Fleet Management** lets you update its configuration centrally
without a redeploy — the collector connects out via **OpAMP** and pulls config. This is how config
targeting, health, and version rollouts work across a fleet of collectors. Enable it on the Kubernetes
integration per the [Fleet Management for Kubernetes doc](https://coralogix.com/docs/user-guides/fleet-management/fleet-remote-config-kubernetes/).

## Tier & cost

- **Tier interaction:** the collector is tier-agnostic; the *signals* it sends inherit their pillar's
  tier behaviour (see each signal's reference). Don't route data to a destination the tier blocks.
- **Customer cost:** the collector adds cluster CPU/memory; egress is the telemetry volume it ships.
  Batching and compression are on by default in the chart. Consider a private link where offered.
- **Coralogix COGS:** ingestion volume from the collector; the collector deployment itself is customer-side.

## AI layers for this signal

- **Layer 1 (no-AI):** the Helm/verify steps above — always works, incl. air-gapped/BYOC.
- **Layer 2 (minimal free AI):** optional — summarize a `CrashLoopBackOff` reason or a collector log
  error into a likely fix (cheap model, low token).
- **Layer 3 (full Olly, paid):** optional — analyze the cluster and propose/deploy a collector config
  autonomously (credit-gated).

## Docs deep-links

- [Kubernetes complete observability — basic configuration](https://coralogix.com/docs/external/telemetry-shippers/otel-integration/k8s-helm/kubernetes-observability/kubernetes-complete-observability-basic-configuration/)
- [Advanced configuration (central tail-sampling, Operator/CRD)](https://coralogix.com/docs/opentelemetry/kubernetes-observability/advanced-configuration/)
- [Fleet Management for Kubernetes](https://coralogix.com/docs/user-guides/fleet-management/fleet-remote-config-kubernetes/)
- [Integration troubleshooting](https://coralogix.com/docs/opentelemetry/kubernetes-observability/troubleshooting/)
- [Coralogix endpoints (region → domain)](https://coralogix.com/docs/integrations/coralogix-endpoints/)

## Sources / evidence

Coralogix `otel-integration` Helm docs (basic + advanced config); Fleet Management for Kubernetes doc;
endpoints doc (verified 2026-07). This reference is the "get a collector into the fleet" entry step;
ongoing fleet health lives in the `cx-system-health` skill (`health-fleet.md`).
