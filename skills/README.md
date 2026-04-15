# cx Skills

A collection of Claude Code skills for the [`cx` Coralogix CLI](https://github.com/coralogix/cx-cli). Install this plugin to give Claude deep knowledge of how to investigate observability data using the CLI.

## Available Skills

| Skill | Description |
|---|---|
| `cx-alerts` | Manage Coralogix alert definitions — list, inspect, create, enable/disable via `cx alerts` |
| `query-logs` | Query and analyze Coralogix logs using DataPrime syntax via `cx logs` |
| `metrics-query` | Investigate production issues by searching metrics, constructing PromQL queries, and analyzing results via `cx metrics` |
| `query-spans` | Query distributed traces and analyze span latency via `cx spans` |
| `telemetry-querying` | Gateway skill for telemetry-driven investigation — decide where to look (metrics, logs, traces, RUM, APM) before querying |

## Installation

Install as a Claude Code plugin:

```bash
cc --plugin-dir /path/to/skills
```

Or add it permanently in your Claude Code settings.

## Requirements

- [`cx` CLI](https://github.com/coralogix/cx-cli) installed and configured with a valid profile (`cx profiles add`)
- Claude Code

## Usage

Once installed, Claude will automatically use the relevant skill when you ask questions like:

- "Investigate why we're seeing high error rates"
- "Check CPU usage for the api service"
- "Find all metrics related to HTTP requests"
- "What was the p99 latency over the last 24h?"
- "Run a PromQL query to find slow services"
- "How much revenue did we process last week?"
- "Why is the checkout page slow for users?"
- "Debug the 500 errors on the payment endpoint"
- "What's the user journey through the signup flow?"
