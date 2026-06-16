---
name: cx-telemetry-querying
description: |
  Use this skill for any question involving telemetry data: "investigate an issue",
  "debug a problem", "find out why something is slow", "check error rates",
  "analyze user behavior", "understand a production incident",
  "query telemetry data", "look at logs", "search logs", "find errors",
  "find stack traces", "filter by severity",
  "check traces", "examine spans", "investigate request latency",
  "debug service-to-service calls", "look up a trace ID",
  "analyze RUM data", "check frontend performance", "frontend errors",
  "Core Web Vitals", "JavaScript exceptions",
  "query metrics", "check CPU usage", "run a PromQL query",
  "check error rate", "look up a metric", "check memory usage",
  "how do I write a DataPrime query", "DataPrime syntax",
  or wants to answer questions using observability data from logs, metrics, traces, RUM, or APM.
metadata:
  version: "0.2.0"
---

# Telemetry Querying Skill

Use this skill as the entry point for any investigation, debugging, or data question that may be answered from telemetry data. It helps you decide **where** the relevant signal lives (metrics, logs, traces, RUM) and tells you **which reference files to load** before querying.

## Loading References

Before querying, load the reference files for the chosen pillar:

| Pillar | Load these files |
|---|---|
| Logs | `references/dataprime-reference.md` + `references/logs-querying.md` |
| Spans / Traces | `references/dataprime-reference.md` + `references/spans-querying.md` |
| Metrics | `references/promql-guidelines.md` + `references/metrics-querying.md` |
| RUM (frontend) | `references/dataprime-reference.md` + `references/logs-querying.md` + `references/rum-querying.md` + `references/rum-fields.md` |
| DataPrime syntax only | `references/dataprime-reference.md` |

---

## Safety

All query commands (`cx logs`, `cx spans`, `cx metrics`, `cx dataprime`, `cx search-fields`) are read-only and work in `--read-only` mode. They never modify data and can be run freely without `--yes`.

---

## Quick Routing Guide

Use this table for obvious cases where one pillar is the clear first choice:

| Question Type | First Choice | Fallback |
|---|---|---|
| UI behavior, page load, frontend errors | RUM | Traces (if backend-related) |
| Endpoint latency, throughput, error rates | Metrics | Traces (for per-request detail) |
| Service-to-service dependencies, request flow | Traces | Logs (for debug output) |
| Specific error messages, stack traces | Logs | Traces (for request context) |
| Infrastructure health (CPU, memory, disk) | Metrics | - |
| Business events (purchases, signups) | Depends - see Discovery Workflow | - |

For **ambiguous questions** (e.g., "How much money did users spend last week?"), the signal could live in any pillar. Follow the Discovery Workflow below.

---

## Discovery Workflow

When the answer could reside in multiple pillars, run discovery in parallel to find the best source.

### Step 1: Search Metrics

Check if a relevant metric exists:

```bash
cx metrics search --name '*transaction*'
cx metrics search --name '*payment*'
cx metrics search --name '*revenue*'
cx metrics search --description "total purchase amount"
```

If a matching metric is found, load `references/promql-guidelines.md` + `references/metrics-querying.md` and continue.

### Step 2: Search Log and Span Fields

Use semantic field search to find relevant DataPrime paths:

```bash
cx search-fields "transaction amount" --dataset logs
cx search-fields "payment total" --dataset spans
cx search-fields "purchase value" --dataset logs --limit 10
```

If you know a concrete value that should appear in the data but don't know which field holds it, use value search instead. It returns the matching field keys alongside sample values, which also lets you infer the field's type (string, numeric, enum, etc.):

```bash
cx search-fields "payment_failed" -s value --dataset logs
cx search-fields "grpc.status.UNAVAILABLE" -s value --dataset spans
cx search-fields "eu-west-1" -s value --dataset all
```

**Requirements:** `cx search-fields` needs a Coralogix API key or OAuth on the active profile. If credentials are missing, prompt the user to run `cx profiles add <name>`.

If matching fields are found:
- For **logs**: load `references/dataprime-reference.md` + `references/logs-querying.md`
- For **spans**: load `references/dataprime-reference.md` + `references/spans-querying.md`

### Step 3: Search the Codebase

When discovery results are ambiguous or you need to validate what a metric/field actually represents, search the codebase:

- Look for metric registration code (e.g., `prometheus.NewCounter`, `metrics.record`)
- Look for log statements that emit the field (e.g., `logger.info("transaction", ...)`)
- Look for span attributes (e.g., `span.setAttribute("purchase.amount", ...)`)

This confirms the semantic meaning and helps you choose the right pillar.

### Step 4: Choose and Query

Based on discovery results, pick the pillar with the clearest signal, load its reference files (see [Loading References](#loading-references)), then query.

---

## Fallback and Pivoting

**If your initial route yields no results, pivot to another pillar.**

Example pivot paths:
- Metrics empty → try traces (per-request data) or logs (event records)
- Logs empty → try traces (structured span attributes) or metrics (aggregated counters)
- Traces empty → try logs (text-based debug output)

Do not stop after one failed attempt. Try at least two pillars before concluding the data does not exist.

---

## CLI Commands Reference

| Command | Purpose | When to Use |
|---|---|---|
| `cx schema` | Output the full command tree as JSON | Discover all available commands and their flags |
| `cx metrics search --name <pattern>` | Find metrics by name | First step for metrics discovery |
| `cx metrics search --description <text>` | Semantic metric search | When you know what you want but not the name |
| `cx search-fields "<text>" --dataset logs` | Find log fields by description | Discovery for log-based questions |
| `cx search-fields "<text>" --dataset spans` | Find span fields by description | Discovery for trace-based questions |
| `cx search-fields "<value>" -s value --dataset logs` | Find log fields that contain a known value | When you know a value but not which log field holds it — also reveals field type from the returned values |
| `cx search-fields "<value>" -s value --dataset spans` | Find span fields that contain a known value | When you know a value but not which span attribute holds it |
| `cx search-fields "<value>" -s value --dataset all` | Same, across logs and spans | When you want to search across both logs and spans at once |
| `cx spans "filter $l.serviceName == '<service>'" --limit 10` | Search spans by service | When investigating a specific service |
| `cx dataprime list` | List DataPrime commands/functions | When building log or span queries |
| `cx dashboards search "<description>"` | Find existing dashboards by natural-language description | Before creating a new dashboard — check if one already exists |
| `cx dashboards query-search --description "<text>"` | Find dashboard widgets whose queries cover a topic | Discover how a topic is already being monitored |
| `cx dashboards query-search --field "<field-path>"` | Find widgets that reference a specific field | Reuse existing PromQL/DataPrime patterns for a known field |

---

## Examples

### Example 1: Business Question (Ambiguous Source)

**Question:** "How much money did people spend on the platform last week?"

**Approach:**
1. Search metrics: `cx metrics search --name '*revenue*'` and `cx metrics search --name '*transaction*'`
2. Search log fields: `cx search-fields "transaction amount" --dataset logs`
3. Search span fields: `cx search-fields "payment total" --dataset spans`
4. If a metric like `payment_total_usd` exists, load metrics references and run a range query
5. If only logs have the data, load logs references and use DataPrime aggregation
6. If traces have `purchase.amount` attribute, load spans references

### Example 2: Latency Question (Clear First Choice)

**Question:** "What's the average latency of the checkout route?"

**Approach:**
1. First try metrics: `cx metrics search --name '*checkout*latency*'` or `cx metrics search --name '*http*duration*'`
2. If a histogram metric exists, load metrics references and use `histogram_quantile`
3. If no metric, fall back to traces: load spans references and aggregate span durations

### Example 3: Frontend Performance (RUM)

**Question:** "Why is the dashboard page loading slowly for users?"

**Approach:**
1. This is clearly a RUM question - load `references/rum-querying.md` + `references/rum-fields.md` + `references/logs-querying.md` + `references/dataprime-reference.md`
2. Query web vitals and page load times
3. If RUM shows backend calls are slow, pivot to spans references for the API calls

### Example 4: Error Investigation (Logs + Traces)

**Question:** "Why are users getting 500 errors on the payment endpoint?"

**Approach:**
1. Check error rate metrics → load metrics references
2. Search for error logs → load logs references
3. Get traces for failed requests → load spans references
4. Cross-reference: find trace IDs in logs, then fetch full traces for root cause

---

## Beyond Investigation

Not every question is answered by querying data. If the user's intent is operational rather than investigative, route to the appropriate workflow skill:

| User Intent | Route To |
|---|---|
| Reducing costs, checking usage, TCO policies | `cx-cost-optimization` |
| Incident triage, SLO breaching, who got paged | `cx-incident-management` |
| Setting up monitoring, webhooks, notifications | `cx-observability-setup` |
| Configuring parsing rules, enrichments, E2M | `cx-data-pipeline` |
| Access audit, API keys, user management | `cx-platform-admin` |
| Creating or managing dashboards | `cx-dashboards` |
| Finding or searching existing dashboards | `cx-search-dashboard` |

---


## Key Principles

- **Load references before querying**: check the [Loading References](#loading-references) table first
- **Discover before querying**: always run search/discovery to find the right source
- **Parallel discovery**: for ambiguous questions, search metrics, logs, and spans concurrently
- **Validate with code**: when unsure what a metric or field represents, check the codebase
- **Pivot on failure**: if one pillar is empty, try another before giving up
