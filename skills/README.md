# cx skills

Use these skills to give your coding agent access to Coralogix observability data — logs, spans, metrics, RUM, and alerts — directly from the CLI.

Supports Claude Code, Cursor, Codex, OpenCode, and [other supported agents](https://github.com/vercel-labs/skills#supported-agents).

## Available skills

### Investigation

| Skill | Description |
|---|---|
| `cx-olly` | Interact with Coralogix's Observability Agent (Olly) - chat, follow-up questions, retrieve generated artifacts |
| `cx-telemetry-querying` | Gateway for all telemetry investigation - logs, spans, metrics, RUM, and DataPrime queries. Loads pillar-specific reference files per query type. |
| `coralogix-docs` | Search and fetch official Coralogix product documentation (`cx docs search`, `cx docs fetch`) — not live tenant data |
| `cx-alerts` | Manage Coralogix alert definitions - list, inspect, create, enable/disable via `cx alerts` |
| `cx-dashboards` | Build and deploy Coralogix dashboards - telemetry discovery, PromQL/DataPrime verification, JSON generation |

### Workflow

| Skill | Description |
|---|---|
| `cx-cost-optimization` | Analyze and reduce Coralogix data costs - usage analysis, TCO policies, retention, archive |
| `cx-cases` | Manage Coralogix Cases - list, inspect, assign/acknowledge/resolve/close, set priority overrides |
| `cx-slos` | Manage SLO definitions - list, inspect, check error budgets, create/update/delete |
| `cx-data-pipeline` | Configure data processing - parsing rules, enrichments, Events2Metrics, recording rules |
| `cx-platform-admin` | Manage access and security - API keys, roles, users, groups, IP access |
| `cx-observability-setup` | Set up monitoring - saved views, webhooks, notifications, integrations |

## Requirements

- [`cx` CLI](https://github.com/coralogix/cx-cli) installed and configured with a valid profile (`cx profiles add`)
- A supported coding agent (Claude Code, Cursor, Codex, etc.)

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

## Usage

Once installed, your agent uses the relevant skill automatically. Example queries:

- "Ask Olly why the checkout service is slow"
- "Investigate why we're seeing high error rates"
- "Check CPU usage for the api service"
- "What was the p99 latency over the last 24h?"
- "Why is the checkout page slow for users?"
- "Debug the 500 errors on the payment endpoint"
- "How can we reduce our Coralogix costs?"
- "Help me triage this case"
- "Set up parsing rules for our new service"
- "Who has access to production?"
- "Configure Slack notifications for critical alerts"
