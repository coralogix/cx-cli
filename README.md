# CX - Coralogix CLI

[![CI](https://github.com/coralogix/cx-cli/actions/workflows/build.yml/badge.svg)](https://github.com/coralogix/cx-cli/actions/workflows/build.yml)
[![Crates.io](https://img.shields.io/crates/v/coralogix-cli)](https://crates.io/crates/coralogix-cli)
[![Homebrew](https://img.shields.io/badge/homebrew-coralogix%2Ftap%2Fcx-blue)](https://github.com/coralogix/homebrew-tap)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

The observability backbone for AI agents and engineering teams.<br/>
Connect your agents to live logs, traces, metrics, dashboards, and alerts so they can investigate incidents, explain what changed, and reason about production with real operational context.

<p align="center">
  <img src="https://github.com/coralogix/cx-cli/raw/master/assets/demo.png" alt="cx logs demo" width="700">
</p>

## What you can do

- Query any signal-logs, metrics, spans, and RUM data-with DataPrime or PromQL, and render results as tables, raw JSON, or a token-efficient format for AI agents.
- Manage the full Coralogix stack from one binary: 33 commands across 11 domains, covering alerts, cases, notifications, IAM, SLOs, dashboards, data pipeline rules, TCO policies, and more.
- Set everything up in one command-`cx init` configures a profile and installs the agent skills in a single guided run, so there is no way to end up with a CLI your agent can't use.
- Run the same command across multiple profiles or regions in a single invocation with multi-profile fan-out.
- Give your AI agent a single entry point to production observability: `cx schema` dumps the entire command tree as JSON so agents can self-discover capabilities without manual documentation.
- Hand agents results instead of a flooded context window: the `toon` output format is token-efficient JSON that auto-spills to a temp file once the payload exceeds 100 KiB, returning a path.
- Find the right log or span field by describing it in natural language.
- Browse the DataPrime language reference offline.
- Plug Coralogix into your AI coding agent with bundled skills for Claude Code, Cursor, Codex, and 40+ more agents.

:::tip[Ready to get started?]
Use the [Quick start guide](docs/quickstart.md) for instant installation.
:::

## Further reading

- [Quick start](docs/quickstart.md)
- [Advanced configuration](docs/configuration.md)
- [Agent skills](skills/README.md)
- [Multi-profile fan-out](docs/multi-profile.md)
- [TOON output format](docs/toon-output.md)
- [Time syntax](docs/time-syntax.md)
- [Architecture](contributing/architecture.md)
- [Development guide](contributing/development.md)

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for the ownership model,
PR review process, and step-by-step guides for adding commands and skills.

## License

Apache-2.0
