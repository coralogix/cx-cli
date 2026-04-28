---
name: telemetry-querying
description: This skill should be used when the user asks to "investigate an issue", "debug a problem", "find out why something is slow", "check error rates", "analyze user behavior", "understand a production incident", "query telemetry data", "look at logs", "check traces", "examine spans", "analyze RUM data", "check frontend performance", "investigate backend latency", "find transaction data", "check payment metrics", "analyze user journeys", or wants to answer questions using observability data from logs, metrics, traces, RUM, or APM — this is the gateway skill for deciding where to look first.
version: 0.1.0
---

# Telemetry Querying Skill

Use this skill as the entry point for any investigation, debugging, or data question that may be answered from telemetry data. This skill helps you decide **where** the relevant signal lives (metrics, logs, traces, RUM, APM) before diving into queries, then delegates to specialized skills for deep exploration.

## Core Principle

**Decide where to look before querying.** Telemetry data is spread across multiple pillars. Choosing the right source first saves time and yields better answers.

---

## Quick Routing Guide

Use this table for obvious cases where one pillar is the clear first choice:

| Question Type | First Choice | Fallback |
|---|---|---|
| UI behavior, page load, frontend errors | RUM | Traces (if backend-related) |
| Endpoint latency, throughput, error rates | Metrics | Traces (for per-request detail) |
| Service-to-service dependencies, request flow | Traces | Logs (for debug output) |
| Specific error messages, stack traces | Logs | Traces (for request context) |
| Infrastructure health (CPU, memory, disk) | Metrics | — |
| Business events (purchases, signups) | Depends — see Discovery Workflow | — |

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

If a matching metric is found, continue with the `metrics-query` skill.

### Step 2: Search Log and Span Fields

Use semantic field search to find relevant DataPrime paths:

```bash
cx search-fields "transaction amount" --dataset logs
cx search-fields "payment total" --dataset spans
cx search-fields "purchase value" --dataset logs --limit 10
```

**Requirements:** `cx search-fields` needs a Coralogix API key or OAuth on the active profile. If credentials are missing, prompt the user to run `cx profiles add`.

If matching fields are found:
- For **logs**: continue with the `query-logs` skill using DataPrime
- For **spans**: continue with the `query-spans` skill

### Step 3: Search the Codebase

When discovery results are ambiguous or you need to validate what a metric/field actually represents, search the codebase:

- Look for metric registration code (e.g., `prometheus.NewCounter`, `metrics.record`)
- Look for log statements that emit the field (e.g., `logger.info("transaction", ...)`)
- Look for span attributes (e.g., `span.setAttribute("purchase.amount", ...)`)

This confirms the semantic meaning and helps you choose the right pillar.

### Step 4: Choose and Query

Based on discovery results, pick the pillar with the clearest signal and delegate to the appropriate skill:

| Pillar | Skill to Use |
|---|---|
| Metrics | `metrics-query` |
| Logs | `query-logs` |
| Traces/Spans | `query-spans` |
| RUM | `rum` |
| APM | APM-specific guidance |

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
| `cx spans "filter $l.serviceName == '<service>'" --limit 10` | Search spans by service | When investigating a specific service |
| `cx dataprime list` | List DataPrime commands/functions | When building log queries |

---

## Examples

### Example 1: Business Question (Ambiguous Source)

**Question:** "How much money did people spend on the platform last week?"

**Approach:**
1. Search metrics: `cx metrics search --name '*revenue*'` and `cx metrics search --name '*transaction*'`
2. Search log fields: `cx search-fields "transaction amount" --dataset logs`
3. Search span fields: `cx search-fields "payment total" --dataset spans`
4. If a metric like `payment_total_usd` exists, use `metrics-query` skill with a range query
5. If only logs have the data, use `query-logs` skill with DataPrime aggregation
6. If traces have `purchase.amount` attribute, use `query-spans` skill

### Example 2: Latency Question (Clear First Choice)

**Question:** "What's the average latency of the checkout route?"

**Approach:**
1. First try metrics: `cx metrics search --name '*checkout*latency*'` or `cx metrics search --name '*http*duration*'`
2. If a histogram metric exists, use `metrics-query` skill with `histogram_quantile`
3. If no metric, fall back to traces: `cx spans "filter $l.serviceName == 'checkout-service'" --limit 10` and aggregate span durations

### Example 3: Frontend Performance (RUM)

**Question:** "Why is the dashboard page loading slowly for users?"

**Approach:**
1. This is clearly a RUM question — frontend page load data
2. Use `rum` skill directly
3. If RUM shows backend calls are slow, pivot to `query-spans` for the API calls

### Example 4: Error Investigation (Logs + Traces)

**Question:** "Why are users getting 500 errors on the payment endpoint?"

**Approach:**
1. Check error rate metrics: `cx metrics search --name '*error*'` → `metrics-query` skill
2. Search for error logs: `cx search-fields "error message" --dataset logs` → `query-logs` skill
3. Get traces for failed requests: `cx spans "filter $l.serviceName == 'payment-service'" --limit 10` → `query-spans` skill
4. Cross-reference: find trace IDs in logs, then fetch full traces for root cause

---

## Beyond Investigation

Not every question is answered by querying data. If the user's intent is operational rather than investigative, route to the appropriate workflow skill:

| User Intent | Route To |
|---|---|
| Reducing costs, checking usage, TCO policies | `cost-optimization` |
| Incident triage, SLO breaching, who got paged | `incident-management` |
| Setting up monitoring, webhooks, notifications | `observability-setup` |
| Configuring parsing rules, enrichments, E2M | `data-pipeline` |
| Access audit, API keys, user management | `platform-admin` |
| Creating or managing dashboards | `create-dashboard` |

---

## Key Principles

- **Discover before querying**: always run search/discovery to find the right source
- **Parallel discovery**: for ambiguous questions, search metrics, logs, and spans concurrently
- **Validate with code**: when unsure what a metric or field represents, check the codebase
- **Pivot on failure**: if one pillar is empty, try another before giving up
- **Delegate to specialists**: once you know the pillar, hand off to the dedicated skill

---

## Related Skills

### Investigation Skills
- **`dataprime`** — DataPrime query language reference (syntax, operators, aggregations, functions)
- **`metrics-query`** — PromQL queries, metric discovery, instant and range queries
- **`query-logs`** — DataPrime log queries, log field exploration
- **`query-spans`** — Trace search, span analysis, distributed tracing
- **`rum`** — Frontend performance, user sessions, page loads
- **`cx-alerts`** — Creating and managing alert definitions
- **`create-dashboard`** — Dashboard creation and management

### Workflow Skills
- **`cost-optimization`** — Analyze and reduce Coralogix data costs
- **`incident-management`** — Incident triage, SLO monitoring, notification verification
- **`data-pipeline`** — Parsing rules, enrichments, E2M, recording rules
- **`platform-admin`** — Access audit, API keys, user and role management
- **`observability-setup`** — Views, webhooks, notifications, integrations setup
