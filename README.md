# cx

The CLI for Coralogix observability. Query logs, metrics, traces, dashboards, and alerts from the terminal — built for humans and AI agents.

## Features

- Query logs with [DataPrime](https://coralogix.com/docs/dataprime-query-language/) syntax
- Query metrics with PromQL (instant and range queries)
- Search and inspect distributed traces
- List and manage dashboards and alerts
- Search metric and log/span fields semantically by natural-language description
- Browse DataPrime language reference offline
- Fan out queries across multiple profiles simultaneously
- AI-agent-optimized output mode with automatic large-result spilling

## Installation

### Shell (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/coralogix/coralogix-cli/master/install.sh | sh
```

You can pin a specific version:

```bash
CX_VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/coralogix/coralogix-cli/master/install.sh | sh
```

### Homebrew (macOS / Linux)

```bash
brew install coralogix/tap/cx
```

### Cargo

```bash
cargo install cx
```

### Pre-built binaries

Download the latest release for your platform from [GitHub Releases](https://github.com/coralogix/coralogix-cli/releases).

### Build from source

```bash
cargo build --release
cp target/release/cx /usr/local/bin/
```

## Quick Start

```bash
# Configure a profile interactively
cx profiles add

# Query logs
cx logs 'source logs | filter $d.severity == "ERROR"'

# Query metrics
cx metrics query 'rate(http_requests_total[5m])'

# Search traces
cx traces search checkout-service --start now-6h

# List dashboards
cx dashboards catalog

# List alerts
cx alerts list --name "payment"
```

## Configuration

Config lives in `~/.cx/`. Run `cx profiles add` to create a profile interactively.

```
~/.cx/
  config.toml              # Global settings (default profile, output format)
  profiles/
    default.toml           # Credentials and region per profile
```

Environment variable overrides: `CX_PROFILE`, `CX_API_KEY`, `CX_REGION`, `OPENAI_API_KEY`.

See [docs/configuration.md](docs/configuration.md) for full reference.

## Commands

| Command | Description |
|---------|-------------|
| `cx profiles` | Manage profiles (list, add, delete, set-default) |
| `cx logs` | Query logs using DataPrime syntax |
| `cx metrics` | Query metrics using PromQL (instant, range, search, get-labels) |
| `cx traces` | Search and inspect distributed traces |
| `cx dashboards` | List and inspect dashboards |
| `cx alerts` | List, inspect, create, enable, and disable alerts |
| `cx search-fields` | Search log/span fields semantically by description |
| `cx dataprime` | Browse DataPrime language reference |
| `cx cleanup` | Remove stale temp result files |

Run `cx <command> --help` for full usage and examples.

### Global Options

```
-p, --profile <PROFILE>       Profile to use (repeat for multi-profile fan-out)
    --api-key <API_KEY>        Override the profile API key
    --region <REGION>          Override the profile region
-o, --output <text|json|agents> Output format
```

## Output Formats

- **`text`** — human-readable tables and formatted output (default)
- **`json`** — pretty-printed raw API responses
- **`agents`** — token-optimized JSON for AI agent workflows, with automatic spilling of large results to temp files

See [docs/agents-output.md](docs/agents-output.md) for the agents format specification.

## Skills

The `skills/` directory contains Claude Code skill plugins for AI-driven observability investigation — alert management, metrics querying, and telemetry triage.

See [skills/README.md](skills/README.md) for installation and usage.

## Further Reading

- [Configuration reference](docs/configuration.md)
- [Agents output format](docs/agents-output.md)
- [Multi-profile fan-out](docs/multi-profile.md)
- [Time syntax](docs/time-syntax.md)
- [Development guide](docs/development.md)

## License

Apache-2.0
