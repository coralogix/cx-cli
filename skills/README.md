# cx skills

Use these skills to give your coding agent access to Coralogix observability data — logs, spans, metrics, RUM, and alerts — directly from the CLI. They teach your agent how to investigate issues by querying Coralogix, without memorizing DataPrime syntax or API endpoints.

Supports Claude Code, Cursor, Codex, OpenCode, and [other supported agents](https://github.com/vercel-labs/skills#supported-agents).

## Available skills

### Investigation

| Skill | Description                                                                                                                                       |
|---|---------------------------------------------------------------------------------------------------------------------------------------------------|
| `cx-olly` | Interact with Coralogix's Observability Agent (Olly) - chat, follow-up questions, retrieve generated artifacts                                    |
| `cx-telemetry-querying` | Gateway for all telemetry investigation - logs, spans, metrics, RUM, and DataPrime queries. Loads pillar-specific reference files per query type. |
| `coralogix-docs` | Search and fetch official Coralogix product documentation (`cx docs search`, `cx docs fetch`) — not live tenant data                              |
| `cx-alerts` | Manage Coralogix alert definitions - list, inspect, create, enable/disable via `cx alerts`                                                        |
| `cx-dashboards` | Build and deploy Coralogix dashboards - telemetry discovery, PromQL/DataPrime verification, JSON generation                                       |
| `cx-infra` | Explore infrastructure resources - discover monitored resource types, list resources, check per-resource data                                     |
| `cx-service-catalog` | Query Service Catalog APM entities - services, databases, operations, JVMs, K8s pods; schema, entities, and aggregated/timeseries data          |
| `cx-ai-center` | Investigate GenAI applications and agents - behavior, prompts/responses, quality, hallucinations, guardrails, token cost - and manage AI Center config |
| `cx-coding-agents` | Analyze AI Center Coding Agents data for Claude Code, Codex, Cursor, Gemini CLI, and Copilot CLI - usage, cost, tokens, sessions, tools, code impact |
| `cx-search-dashboard` | Find existing dashboards and widgets by natural-language description or by the fields they query |

### Workflow

| Skill | Description |
|---|---|
| `cx-cost-optimization` | Analyze and reduce Coralogix data costs - usage analysis, TCO policies, retention, archive |
| `cx-cases` | Manage Coralogix Cases - list, inspect, assign/acknowledge/resolve/close, set priority overrides |
| `cx-slos` | Manage SLO definitions - list, inspect, check error budgets, create/update/delete |
| `cx-data-pipeline` | Configure data processing - parsing rules, enrichments, Events2Metrics, recording rules |
| `cx-platform-admin` | Manage access and security - API keys, roles, users, groups, IP access |
| `cx-observability-setup` | Set up monitoring - saved views, webhooks, notifications, integrations |
| `cx-cli` | Cross-cutting `cx` behavior that applies to every command, such as update notifications |

## Requirements

- [`cx` CLI](https://github.com/coralogix/cx-cli) installed and configured with a valid profile — `cx init` does both
- Node.js (`npx`), which the installer runs through
- A supported coding agent (Claude Code, Cursor, Codex, etc.)

## Installation

`cx init` installs these skills as part of onboarding, so a normal setup needs
nothing from this page. Use `cx skills install` when you want to install them on
their own, or to **update** them.

`cx skills` is a **local** command: it needs no API credentials and is exempt from
the risky-command confirmation. The global options that show up in
`cx skills --help` — `--profile`, `--api-key`, `--region`, `-o`/`--output`,
`--yes`, `--read-only`, `--no-console-link` — are inherited from the root command
and have no effect here. `install` is its only subcommand.

### Default install

```bash
cx skills install
```

It asks one question and nothing else:

```
? Where should the agent skills be installed?
> Global (~/) - available in every project
  Local (./) - this project only
[Skills teach coding agents (Claude Code, Cursor, Codex, ...) how to use cx.]
```

Then it installs every skill, detects your coding agents automatically, and
replaces the installer's verbose output with a one-line summary:

```
✓ Installed 14 cx agent skills (global). Browse them at: https://skills.sh/coralogix/cx-cli
```

### Flags

| Flag | Purpose |
|---|---|
| `--global` | Install into `~/`, available in every project. Skips the scope question. Conflicts with `--local` and `--interactive`. |
| `--local` | Install into `./`, this project only. Skips the scope question. Conflicts with `--interactive`. |
| `--agent <NAME>` | Install for specific agents rather than letting the installer auto-detect. Repeatable. Conflicts with `--interactive`. |
| `--interactive` | Walk the installer's own full flow — choose individual skills, agents, scope, and install method — with its complete output, risk table included. Cannot be combined with any of the flags above. |
| `-h` / `--help` | `--help` prints the full reference; `-h` prints a short summary. |

```bash
cx skills install --global                                  # no questions asked
cx skills install --local --agent claude-code --agent cursor
cx skills install --interactive
```

### When it can't install

Both failure modes name the fix, and neither leaves a half-installed state:

- **No Node.js** — `Node.js/npx is required to install the cx agent skills.`
  Install Node.js and rerun, or skip the skills step.
- **No scope and no terminal** (CI, containers) —
  `no skills install scope - pass --global or --local`. There is nobody to answer
  the prompt, so name the scope explicitly.

A failed skills install never blocks onboarding: inside `cx init` it downgrades to
a warning, and `cx` itself stays fully usable without skills.

On Windows, `npx` resolves through its `.cmd` shim, so no extra setup is needed.

### Updating your skills

`cx skills install` always reinstalls, which is how you pull in skills that have
changed since you set up. (`cx init` deliberately does not — it skips the step
when skills are already present, so re-running it never overwrites anything.) If
you're not sure whether yours are current, run:

```bash
cx skills install
```

It prints what it found before replacing it, for example:

```
cx agent skills already installed (14 skills: cx-alerts, cx-cases, cx-dashboards, ...) - reinstalling to update.
```

Ask your coding agent to "update my Coralogix skills" and this is the command it
should reach for.

### Installing without the CLI

The CLI wraps the [`skills`](https://github.com/vercel-labs/skills) installer, which
you can also drive directly:

```bash
npx skills add coralogix/cx-cli/skills                         # all skills
npx skills add coralogix/cx-cli/skills --skill cx-telemetry-querying --skill cx-alerts
npx skills add coralogix/cx-cli/skills -g                      # global
npx skills add coralogix/cx-cli/skills -a claude-code -g -y    # a specific agent, unattended
```

Browse the published bundle at [skills.sh/coralogix/cx-cli](https://skills.sh/coralogix/cx-cli).

## Usage

Once installed, your agent uses the relevant skill automatically. Example queries:

- "Ask Olly why the checkout service is slow"
- "Investigate why we're seeing high error rates"
- "Check CPU usage for the api service"
- "What was the p99 latency over the last 24h?"
- "Why is the checkout page slow for users?"
- "Debug the 500 errors on the payment endpoint"
- "Which of our EC2 instances were unhealthy this week?"
- "Show me the top 5 services by p99 latency in the last hour"
- "How can we reduce our Coralogix costs?"
- "Help me triage this case"
- "Set up parsing rules for our new service"
- "Who has access to production?"
- "Configure Slack notifications for critical alerts"
