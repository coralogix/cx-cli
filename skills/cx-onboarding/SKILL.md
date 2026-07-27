---
name: cx-onboarding
description: |
  Use this skill to get telemetry INTO Coralogix from a new or partially-instrumented
  environment — the setup/instrumentation side of onboarding, not querying existing data.
  Trigger when the user asks to "onboard to Coralogix", "set up Coralogix", "send data to Coralogix",
  "instrument my app", "install the Coralogix collector", "deploy the OpenTelemetry collector",
  "ship logs to Coralogix", "send traces / APM to Coralogix", "send metrics to Coralogix",
  "set up RUM", "connect Kubernetes to Coralogix", "get started with Coralogix",
  "add a new service to Coralogix", "set up Fleet Management", "configure the OTel agent",
  "no data yet after I just set this up", "verify my new integration is sending data",
  or wants to stand up observability on an environment and see first data land.
  For "data used to flow and stopped", "what telemetry am I missing", or "is my data healthy
  enough for this experience", route to `cx-system-health` instead.
metadata:
  version: "0.1.0"
---

# Coralogix Onboarding Skill

Use this skill as the **entry point for getting telemetry into Coralogix** — deploying the
collector, instrumenting a service, and confirming data lands. It is the *instrumentation /
data-in* counterpart to `cx-telemetry-querying` (which investigates data that is **already**
flowing). If the user wants to *analyze* existing telemetry, route to `cx-telemetry-querying`
instead.

> **Why CLI-first for onboarding:** the user is already in the terminal with their own tools
> (`kubectl`, `helm`, `aws`, their code). This skill guides instrumentation from where they
> already are, then verifies the result with read-only `cx` queries. Troubleshooting a *degraded*
> experience is UI-first; standing up instrumentation is CLI-first.

---

## How this skill works

1. **Identify the signal(s)** the user wants to onboard (collector, logs, spans/APM, metrics, RUM, AI Center).
2. **Load the matching reference file** from the table below — do this *before* giving instructions.
3. **Walk the prerequisites in order.** Each reference encodes the format and required params for
   that signal. Do not skip ahead — a missing prerequisite is the #1 cause of "no data".
4. **Verify data landed** with a read-only `cx` query (see [Verification](#verification-always-close-the-loop)).
5. **Only mark a step done when data is confirmed in Coralogix**, not when the config was applied.

## Loading references

| The user wants to… | Load this reference | Status |
|---|---|---|
| Deploy / manage the collector, connect Kubernetes, set up Fleet Management | `references/onboarding-fleet.md` | **Full** |
| Send infrastructure & custom **metrics**, light up Infra Explorer | `references/onboarding-metrics-infra.md` | **Full** |
| Send **traces / spans / APM** for a service | `references/onboarding-apm-spans.md` | **Full** |
| Ship **logs** | `references/onboarding-logs.md` | Stub — PM to complete |
| Set up **RUM** (browser / mobile) | `references/onboarding-rum.md` | Stub — PM to complete |
| Send **AI Center** / LLM telemetry | `references/onboarding-ai-center.md` | Stub — PM to complete |

Each reference follows the shared **onboarding reference template**
(`contributing/onboarding-reference-md-template.md`) so every signal has the same shape:
prerequisites → minimal config → verify → common failures → tier & cost → docs deep-links.

---

## Global prerequisites (check once, before any signal)

Confirm these before instrumenting anything. Most "no data" cases fail here.

1. **`cx` CLI installed and a profile configured.**
   ```bash
   cx profiles list            # is there a profile?
   cx profiles add <name>      # if not: prompts for region + Send-Your-Data API key
   ```
   The profile's **region** determines the ingress endpoint. Never hardcode a region — read it
   from the profile. Region → endpoint mapping is in the
   [Coralogix endpoints doc](https://coralogix.com/docs/integrations/coralogix-endpoints/).

2. **A Send-Your-Data API key.** Ingestion authenticates with
   `Authorization: Bearer <send-your-data-api-key>` — this is a *different* key from the query API
   key. See [Send-Your-Data API key](https://coralogix.com/docs/user-guides/account-management/api-keys/send-your-data-api-key/).

3. **The OTLP endpoint pattern.** All native ingestion is OpenTelemetry over gRPC:
   `ingress.<coralogix-domain>:443`. Resolve `<coralogix-domain>` from the profile region.

4. **Application & subsystem naming.** Every data point is tagged with an **application name** and a
   **subsystem name** (set via resource attributes `cx.application.name` / `cx.subsystem.name`, or the
   OTLP metadata headers). Decide these before instrumenting — they are how the data is found later.
   See [Application and subsystem names](https://coralogix.com/docs/user-guides/account-management/account-settings/application-and-subsystem-names/).

> ### ⚠️ The single most common mistake: format, not credentials
> Coralogix's OTLP endpoints expect **OpenTelemetry protobuf over gRPC**, *not* arbitrary plain text.
> Sending a raw text payload to the OTLP endpoint fails silently or is rejected — this is exactly the
> trap that cost a full onboarding cycle before switching to protobuf. **Always confirm the exporter
> is emitting OTLP protobuf** (the OTel SDKs and the Coralogix collector do this by default; hand-rolled
> shippers often do not). Each per-signal reference states its required format and params explicitly —
> read them first.

---

## Recommended onboarding order

Signals are independent, but this order builds confidence fastest and matches how real onboardings
converge (each step verifies before the next):

1. **Collector / Fleet** — get the OTel collector running and reporting (`onboarding-fleet.md`). This is
   the transport for everything else in Kubernetes/host environments.
2. **Logs** — usually the first signal customers care about (`onboarding-logs.md`).
3. **Spans / APM** — service traces (`onboarding-apm-spans.md`).
4. **Metrics / Infra** — infra + custom metrics, Infra Explorer (`onboarding-metrics-infra.md`).
5. **RUM** — front-end / real-user monitoring (`onboarding-rum.md`).
6. **AI Center** — LLM/agent telemetry (`onboarding-ai-center.md`).

For SDK-based (non-Kubernetes) apps, steps 2–5 can be done directly from the app's OTel exporter and
step 1 is optional. Let the user's environment decide — ask before assuming Kubernetes.

---

## Verification — always close the loop

Instrumentation is not "done" until data is queryable. After each signal, verify with read-only
`cx` queries (these consume **no** ingestion quota and no AI Units):

```bash
# Logs landed?
cx logs "filter \$l.applicationname == '<app>'" --start now-15m --limit 5

# Spans landed?
cx spans "filter \$l.serviceName == '<service>'" --start now-15m --limit 5

# Metric present?
cx metrics search --name '*<metric>*'
```

If nothing appears after ~5–10 minutes: extend the time range, re-check the app/subsystem name used,
confirm the endpoint region matches the profile, and confirm the exporter is OTLP protobuf. For deeper
query help, hand off to `cx-telemetry-querying`.

---

## AI usage & no-AI path (required posture)

This skill runs **inside the customer's own coding agent**, so its guidance and `cx` commands
**consume no Coralogix AI Units** — the cost is borne by whatever agent the user runs it in. AI Units
are only consumed if a step explicitly calls the **Olly API** (e.g. "ask Olly to explain this failure").

Design every assist across three layers:

- **Layer 1 — no-AI path (always works):** the deterministic steps + verification queries in the
  reference files. Fully usable in air-gapped / BYOC / high-security environments and when a user is
  out of AI Units. This is the default and must never depend on AI.
- **Layer 2 — minimal free AI (optional):** a low-token assist (e.g. summarize a collector error,
  pick the right fix for a verdict) using a cheap model. If a step absorbs tokens at Coralogix's cost,
  that cost must be explicit in the feature's COGS.
- **Layer 3 — full Olly (paid, credit-gated):** deep, autonomous help (analyze the whole environment,
  propose and deploy a collector config). Available only if the customer has AI credits; surface a
  consumption tooltip, and fall back to Layer 1 when units are exhausted.

---

## Safety

Onboarding is **not** read-only. It deploys collectors, edits Helm/values files, sets env vars, and
changes the user's code or cluster. Treat these as mutating:

- **State what will change before running it** (which files, which cluster/namespace).
- Prefer `helm upgrade --install ... --dry-run` / showing a diff before applying.
- Instrumentation edits happen in the **user's own repo/cluster** — never invent credentials or push
  changes without confirmation.
- The **verification** `cx logs`/`cx spans`/`cx metrics` queries are read-only and safe to run freely.

---

## Key principles

- **Load the reference before instructing** — never improvise a signal's prerequisites.
- **Format first, credentials second** — most failures are OTLP-protobuf / param issues, not auth.
- **Decide app + subsystem names up front** — they are how the data is found later.
- **Verify with a query before declaring success** — config applied ≠ data landing.
- **Read region from the profile** — never hardcode an ingress endpoint.
- **Content is contributable** — each signal's steps live in a per-product MD a PM can own and refine.

---

## Additional resources

### Reference files

- **`references/onboarding-fleet.md`** — deploy the OTel collector on Kubernetes/hosts; Fleet Management remote config (OpAMP).
- **`references/onboarding-metrics-infra.md`** — infra + custom metrics via OTLP/Prometheus; Infra Explorer.
- **`references/onboarding-apm-spans.md`** — service traces / APM via OpenTelemetry.
- **`references/onboarding-logs.md`** — log shipping (stub for a PM to complete).
- **`references/onboarding-rum.md`** — Real User Monitoring (stub for a PM to complete).
- **`references/onboarding-ai-center.md`** — AI Center / LLM telemetry (stub for a PM to complete).

### Authoring standard

- **`contributing/onboarding-reference-md-template.md`** — the benchmark template every per-product reference follows.

---

## Related skills

> **The onboarding ↔ health loop.** `cx-system-health` verifies whether a customer's telemetry meets the
> conditions an experience/extension needs. When it finds a **gap**, it routes back *here* to
> instrument the missing piece (the discovery-gated activation path). So the two skills form a loop:
> onboard → verify health/coverage → gap → onboard the gap.

| User intent | Route to |
|---|---|
| Data used to flow and stopped; is my data healthy/complete enough for an experience | `cx-system-health` |
| Analyze / investigate data that already flows | `cx-telemetry-querying` |
| Look up exact product docs | `coralogix-docs` |
| Configure saved views, webhooks, notifications, integrations/extensions | `cx-observability-setup` |
| Build dashboards for the newly onboarded data | `cx-dashboards` |
| Set up alerts on the new data | `cx-alerts` |
