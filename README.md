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

> **Ready to get started?** Use the [Quick start guide](docs/quickstart.md) for instant installation.

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

## License and Terms

Apache-2.0
Use of the Coralogix services through this CLI is governed by the [Coralogix Terms and Conditions](https://coralogix.com/terms-conditions/), or your separate written agreement with Coralogix where applicable, including when you use the CLI internally or incorporate it into products and services that you make available to your own customers and end users, except to the extent a specific component or dependency is covered by a different license as indicated in that component's LICENSE file.

### DataPrime query validation and diagnostics

`cx logs`, `cx spans`, and `cx dataprime query` reject unknown schema fields by
default. Use `--strict-fields-validation=false` to allow unknown fields with
compile warnings. `--strict-fields-validation` (or `=true`) enables validation
explicitly. This CLI default differs from the public API's permissive default.

All API warnings are printed to stderr as complete JSON envelopes, including
limit warnings, archive warnings, and warnings preceding a failed query.

Add `--extended-information` to include query information on stdout:

```bash
cx logs 'limit 10' --extended-information -o json
cx spans 'limit 10' --strict-fields-validation=false --extended-information -o json
cx dataprime query 'source logs | limit 10' --extended-information -o json
```

For one profile, the extended JSON/TOON response is `{ "results": [...], "queryId": "...",
"warnings": [...], "statistics": {...} }`. Missing query IDs/statistics are
`null`; absent warnings are `[]`. Results keep their existing normalization.

For multiple profiles, the response is `{ "results": [...], "queries": [...] }`.
Each query entry contains `profile`, `queryId`, `warnings`, and `statistics`;
result rows retain their profile tags. IDs and statistics are not combined.

Text mode keeps the existing result rendering and appends a JSON diagnostics
object after a blank line. For one profile, this object contains `queryId`,
`warnings`, and `statistics`; for multiple profiles it contains `queries`.
JSON mode returns the combined envelope; TOON mode encodes that envelope as TOON.
Extended output stays on stdout, bypassing result spilling.
Without the flag, the existing stdout format and spilling behavior are preserved.
Query failures still return a nonzero exit code.
