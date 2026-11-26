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
| `cx-infra` | Explore infrastructure resources - discover resource types and filterable attributes, list and filter resources, check per-resource data                                     |
| `cx-service-catalog` | Query Service Catalog APM entities - services, databases, operations, JVMs, K8s pods; schema, entities, aggregated/timeseries data, and insights |
| `cx-product-analytics` | Analyze user behavior from RUM - top pages, clicks and rage clicks, funnels and drop-off, page paths, heatmaps; manage saved funnels, pathways, heatmaps |
| `cx-profiling` | Query continuous profiling - top CPU/memory services, profile types, filters, usage over time by pod/label, flame graphs and hot functions |
| `cx-ai-center` | Investigate GenAI applications and agents - behavior, prompts/responses, quality, hallucinations, guardrails, token cost - and manage AI Center config |
| `cx-coding-agents` | Analyze AI Center Coding Agents data for Claude Code, Codex, Cursor, Gemini CLI, and Copilot CLI - usage, cost, tokens, sessions, tools, code impact |
| `cx-search-dashboard` | Find existing dashboards and widgets by natural-language description or by the fields they query |
| `cx-datasets` | Discover queryable datasets - fully qualified `<dataspace>/<dataset>` names for DataPrime sources, with DPXL filtering, plus each dataset's fields and sample values |

### Workflow

| Skill | Description |
|---|---|
| `cx-cost-optimization` | Analyze and reduce Coralogix data costs - usage analysis, TCO policies, retention, archive |
| `cx-cases` | Manage Coralogix Cases - list, inspect, assign/acknowledge/resolve/close, set priority overrides |
| `cx-slos` | Manage SLO definitions - list, inspect, check error budgets, create/update/delete |
| `cx-data-pipeline` | Configure data processing - parsing rules, enrichments, Events2Metrics, recording rules |
| `cx-platform-admin` | Manage access and security - API keys, roles, users, groups, IP access |
| `cx-observability-setup` | Set up monitoring - saved views, webhooks, notifications, integrations |
| `cx-rum-setup` | Add RUM to a browser or mobile app - RUM API key, SDK install and init, verify events arrive |
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
  Install Node.js, then run `cx skills install --global` (or `--local`); the
  message names the command with the scope you already chose.
- **No scope and no terminal** (CI, containers) —
  `no skills install scope - pass --global or --local`. There is nobody to answer
  the prompt, so name the scope explicitly.

A failed skills install never blocks onboarding: inside `cx init` it downgrades to
a warning. Coding agents use `cx` much better with the skills, so install Node.js
and run `cx skills install --global` (or `--local`) afterwards.

On Windows, `npx` resolves through its `.cmd` shim, so no extra setup is needed.

### Updating your skills

The skills always match your cx version: `cx skills install` installs them from
the release tag of the cx you run (for example `v0.1.33`), not from the default
branch. So you get newer skills by upgrading cx.

**After a cx upgrade, cx updates global skills by itself.** The first cx command
after the upgrade (any command) starts the update in the background and prints
one line:

```
[cx update] refreshing cx agent skills for cx 0.1.33 in the background
```

The command doesn't wait for it. Your agent gets the new skills in its next
session. This covers global skills installed with `cx skills install --global`,
`cx init`, an older cx, or `npx` directly. It needs `npx` on your `PATH`. If the
update can't run or fails, cx prints the command to run instead, and tries again
the next day. Local (per-project) skills are not updated automatically.

To turn it off, set `auto_update_skills = false` in `~/.cx/config.toml`, or set
`CX_NO_SKILLS_AUTO_UPDATE=1`. cx then only tells you when your skills are older
than cx.

To update by hand at any time, run:

```bash
cx skills install
```

It prints what it found before replacing it, for example:

```
cx agent skills already installed (14 skills: cx-alerts, cx-cases, cx-dashboards, ...) - reinstalling to update.
```

`cx init` deliberately doesn't reinstall: it skips the step when skills are
already present, so re-running it never overwrites anything. Ask your coding agent
to "update my Coralogix skills" and `cx skills install` is the command it should
reach for.

### Installing without the CLI

The CLI wraps the [`skills`](https://github.com/vercel-labs/skills) installer, which
you can also drive directly. Without a `#v<version>` tag, these install the
newest skills from the default branch, which can mention commands your cx version
doesn't have yet. cx doesn't undo such an install: it updates those skills at your
next cx upgrade.

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
- "What is the checkout service spending its CPU on?"
- "How can we reduce our Coralogix costs?"
- "Help me triage this case"
- "Set up parsing rules for our new service"
- "Add Coralogix RUM to my React app"
- "Who has access to production?"
- "Configure Slack notifications for critical alerts"
