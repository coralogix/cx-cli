# cx Skills

A collection of agent skills for the [`cx` Coralogix CLI](https://github.com/coralogix/cx-cli). Install these skills to give your coding agent deep knowledge of how to investigate observability data using the CLI.

Supports **Claude Code**, **Cursor**, **Codex**, **OpenCode**, and [40+ more agents](https://github.com/vercel-labs/skills#supported-agents).

## Available Skills

### Investigation

| Skill | Description |
|---|---|
| `cx-olly` | Interact with Coralogix's AI assistant (Olly) - chat, follow-up questions, retrieve generated artifacts |
| `cx-telemetry-querying` | Gateway skill for telemetry-driven investigation - decide where to look (metrics, logs, traces, RUM, APM) before querying |
| `cx-query-logs` | Query and analyze Coralogix logs using DataPrime syntax via `cx logs` |
| `cx-query-spans` | Query distributed traces and analyze span latency via `cx spans` |
| `cx-metrics-query` | Investigate production issues by searching metrics, constructing PromQL queries, and analyzing results via `cx metrics` |
| `cx-rum` | Query and analyze Real User Monitoring data - frontend errors, web vitals, user interactions, page performance via `cx logs` |
| `cx-dataprime` | DataPrime query language reference - syntax, commands, operators, aggregations, text extraction |
| `cx-telemetry-querying` | Gateway for all telemetry investigation — logs, spans, metrics, RUM, and DataPrime queries. Loads pillar-specific reference files (DataPrime syntax, PromQL guidelines, logs/spans/metrics/RUM how-tos) per query type. |
| `cx-alerts` | Manage Coralogix alert definitions - list, inspect, create, enable/disable via `cx alerts` |
| `cx-create-dashboard` | Build and deploy Coralogix dashboards - telemetry discovery, PromQL/DataPrime verification, JSON generation |

### Workflow

| Skill | Description |
|---|---|
| `cx-cost-optimization` | Analyze and reduce Coralogix data costs - usage analysis, TCO policies, retention, quotas, archive |
| `cx-incident-management` | Triage incidents end-to-end - incidents, SLOs, alerts, notification verification |
| `cx-data-pipeline` | Configure data processing - parsing rules, enrichments, Events2Metrics, recording rules |
| `cx-platform-admin` | Manage access and security - API keys, roles, users, groups, SAML, IP access |
| `cx-observability-setup` | Set up monitoring - saved views, webhooks, notifications, integrations |

## Installation

Install all skills:

```bash
npx skills add coralogix/cx-cli
```

Install specific skills:

```bash
npx skills add coralogix/cx-cli --skill query-logs --skill dataprime
```

Install globally (available across all projects):

```bash
npx skills add coralogix/cx-cli -g
```

Install for a specific agent:

```bash
npx skills add coralogix/cx-cli -a claude-code -g -y
```

## Requirements

- [`cx` CLI](https://github.com/coralogix/cx-cli) installed and configured with a valid profile (`cx profiles add`)
- A supported coding agent (Claude Code, Cursor, Codex, etc.)

## Usage

Once installed, your agent will automatically use the relevant skill when you ask questions like:

- "Ask Olly why the checkout service is slow"
- "Investigate why we're seeing high error rates"
- "Check CPU usage for the api service"
- "What was the p99 latency over the last 24h?"
- "Why is the checkout page slow for users?"
- "Debug the 500 errors on the payment endpoint"
- "How can we reduce our Coralogix costs?"
- "Help me triage this incident"
- "Set up parsing rules for our new service"
- "Who has access to production?"
- "Configure Slack notifications for critical alerts"
