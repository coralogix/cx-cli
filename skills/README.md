# cx Skills

A collection of agent skills for the [`cx` Coralogix CLI](https://github.com/coralogix/coralogix-cli). Install these skills to give your coding agent deep knowledge of how to investigate observability data using the CLI.

Supports **Claude Code**, **Cursor**, **Codex**, **OpenCode**, and [40+ more agents](https://github.com/vercel-labs/skills#supported-agents).

## Available Skills

| Skill | Description |
|---|---|
| `cx-alerts` | Manage Coralogix alert definitions — list, inspect, create, enable/disable via `cx alerts` |
| `dataprime` | DataPrime query language reference — syntax, commands, operators, aggregations, text extraction |
| `metrics-query` | Investigate production issues by searching metrics, constructing PromQL queries, and analyzing results via `cx metrics` |
| `query-logs` | Query and analyze Coralogix logs using DataPrime syntax via `cx logs` |
| `query-spans` | Query distributed traces and analyze span latency via `cx spans` |
| `rum` | Query and analyze Real User Monitoring data — frontend errors, web vitals, user interactions, page performance via `cx logs` |
| `telemetry-querying` | Gateway skill for telemetry-driven investigation — decide where to look (metrics, logs, traces, RUM, APM) before querying |

## Installation

Install all skills:

```bash
npx skills add coralogix/coralogix-cli
```

Install specific skills:

```bash
npx skills add coralogix/coralogix-cli --skill query-logs --skill dataprime
```

Install globally (available across all projects):

```bash
npx skills add coralogix/coralogix-cli -g
```

Install for a specific agent:

```bash
npx skills add coralogix/coralogix-cli -a claude-code -g -y
```

## Requirements

- [`cx` CLI](https://github.com/coralogix/coralogix-cli) installed and configured with a valid profile (`cx profiles add`)
- A supported coding agent (Claude Code, Cursor, Codex, etc.)

## Usage

Once installed, your agent will automatically use the relevant skill when you ask questions like:

- "Investigate why we're seeing high error rates"
- "Check CPU usage for the api service"
- "Find all metrics related to HTTP requests"
- "What was the p99 latency over the last 24h?"
- "Run a PromQL query to find slow services"
- "How much revenue did we process last week?"
- "Why is the checkout page slow for users?"
- "Debug the 500 errors on the payment endpoint"
- "What's the user journey through the signup flow?"
