# cx

The Coralogix CLI. Query logs, metrics, spans, and Real User Monitoring (RUM) data from the terminal using DataPrime and PromQL—built for humans and AI agents.

## What you can do

- Query any signal—logs, metrics, spans, and RUM data—with DataPrime or PromQL, and render results as tables, raw JSON, or a token-efficient format for AI agents.
- List, inspect, and manage dashboards and alerts.
- Run the same query across multiple profiles or regions in a single command.
- Find the right log or span field by describing it in natural language.
- Browse the DataPrime language reference offline.
- Plug Coralogix into your AI coding agent with bundled skills for Claude Code, Cursor, Codex, and 40+ more agents.

## What makes cx different

- DataPrime and PromQL at the terminal—Coralogix's proprietary query languages work end-to-end without leaving the shell.
- Multi-profile fan-out with `-p prod-eu -p prod-us <command>`—run one query across multiple accounts or regions in a single invocation, with rows tagged by profile.
- `agents` output format—token-efficient JSON that auto-spills to a temp file once the serialized payload exceeds 100 KiB, so AI agents get a path instead of a flooded context window.
- Semantic field search—find the right log or span field by describing it in natural language.
- Bundled skills for Claude Code, Cursor, Codex, OpenCode, and 40+ more agents, distributed via `npx skills add`.

## Installation

### macOS and Linux

Install the latest release with the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/coralogix/cx-cli/master/install.sh | sh
```

Pin a specific version:

```bash
CX_VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/coralogix/cx-cli/master/install.sh | sh
```

### Homebrew

```bash
brew install coralogix/tap/cx
```

### Cargo

```bash
cargo install cx
```

### Pre-built binaries

Download the latest release for your platform from [GitHub Releases](https://github.com/coralogix/cx-cli/releases).

### Build from source

```bash
cargo build --release
cp target/release/cx /usr/local/bin/
```

## Quick start

Follow these steps to go from a fresh install to a working query.

1. Create a profile. `cx profiles add` opens an interactive prompt for region, credentials, and default output format:

    ```bash
    cx profiles add
    ```

2. Query logs. The positional argument is a DataPrime query:

    ```bash
    cx logs 'filter $m.severity == "ERROR"'
    ```

3. Query metrics. `cx metrics query` takes a PromQL expression:

    ```bash
    cx metrics query 'rate(http_requests_total[5m])'
    ```

4. Search distributed spans. The positional argument is a DataPrime filter; `source spans` is prepended automatically:

    ```bash
    cx spans 'filter $l.serviceName == "checkout"' --start now-2h --limit 50
    ```

5. List dashboards to confirm the API is reachable:

    ```bash
    cx dashboards catalog
    ```

Run `cx <command> --help` for full syntax and examples on any command.

## Commands

| Command | Purpose |
|---|---|
| `cx profiles` | Manage profiles: `list`, `add`, `delete`, `set-default` |
| `cx logs` | Query logs using DataPrime |
| `cx spans` | Query distributed spans |
| `cx metrics` | Query metrics using PromQL: `query`, `query-range`, `search`, `get-labels` |
| `cx dashboards` | List and inspect dashboards: `catalog`, `get` |
| `cx alerts` | Manage alerts: `list`, `get`, `create`, `enable`, `disable` |
| `cx search-fields` | Find log or span fields by description |
| `cx dataprime` | Browse the DataPrime language reference offline, or run raw queries: `list`, `show`, `query` |
| `cx cleanup` | Remove `cx_results*` temp files older than 30 minutes |

### Global options

```
-p, --profile <PROFILE>      Profile to use. Repeat to fan out across multiple profiles.
    --api-key <API_KEY>      Override the profile API key
    --region <REGION>        Override the profile region
-o, --output <FORMAT>        text | json | agents (default: text)
```

## Configuration

Configuration lives in `~/.cx/`:

```
~/.cx/
  config.toml              # Global settings
  profiles/
    default.toml           # Credentials and region per profile
```

Credentials are stored in the OS keyring on macOS (Keychain) and Windows (Credential Manager). On Linux, keyring support (Secret Service) requires a glibc build; the default install script and release binaries use musl, which has no keyring backend—credentials fall back to file storage. If you need keyring support on Linux, build from source with a glibc toolchain.

Environment variables override profile settings: `CX_PROFILE`, `CX_API_KEY`, `CX_REGION`, `OPENAI_API_KEY`.

See [docs/configuration.md](docs/configuration.md) for the full reference.

## Output formats

Choose an output format with `-o` or by setting the profile default.

- `text`—human-readable tables with color. Default.
- `json`—raw, pretty-printed API responses for scripting.
- `agents`—token-efficient format for AI agents. Large responses automatically spill to a temporary file and the path is returned.

See [docs/agents-output.md](docs/agents-output.md) for the `agents` format specification.

## AI agent skills

`cx` ships a companion skill bundle for Claude Code, Cursor, Codex, OpenCode, and [40+ other agents](https://github.com/vercel-labs/skills#supported-agents). The skills teach your agent how to investigate issues by querying Coralogix—without memorizing DataPrime syntax or API endpoints.

Install all skills:

```bash
npx skills add coralogix/cx-cli
```

Install selected skills:

```bash
npx skills add coralogix/cx-cli --skill query-logs --skill dataprime
```

Install globally for all projects:

```bash
npx skills add coralogix/cx-cli -g
```

Available skills: `query-logs`, `query-spans`, `metrics-query`, `cx-alerts`, `dataprime`, `rum`, `telemetry-querying`. See [skills/README.md](skills/README.md) for per-skill usage.

## Multi-profile fan-out

Repeat `-p` to run a command across multiple profiles in parallel. Results are merged and tagged with the profile name:

```bash
cx -p prod-eu -p prod-us logs 'filter $m.severity == "ERROR"'
```

See [docs/multi-profile.md](docs/multi-profile.md) for more examples.

## Migrating from cxctl

`cx` replaces the older Scala-based `cxctl`. If you are looking for documentation on the legacy tool, see the [Coralogix CLI (legacy) docs](https://coralogix.com/docs/developer-portal/infrastructure-as-code/cli/coralogix-cli/). `cx` does not currently cover all legacy surfaces, including LiveTail, SAML management, and account invite flows.

## Further reading

- [Configuration](docs/configuration.md)
- [Agents output format](docs/agents-output.md)
- [Multi-profile fan-out](docs/multi-profile.md)
- [Time syntax](docs/time-syntax.md)
- [Architecture](docs/architecture.md)
- [Development guide](docs/development.md)

## License

Apache-2.0
