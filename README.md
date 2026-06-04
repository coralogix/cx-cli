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
- Manage the full Coralogix stack: alerts, incidents, notifications, IAM, SLOs, dashboards, data pipeline rules, TCO policies, and more.
- Run the same command across multiple profiles or regions in a single invocation with multi-profile fan-out.
- Give your AI agent a single entry point to production observability: `cx schema` dumps the entire command tree as JSON so agents can self-discover capabilities without manual documentation.
- Find the right log or span field by describing it in natural language.
- Browse the DataPrime language reference offline.
- Plug Coralogix into your AI coding agent with bundled skills for Claude Code, Cursor, Codex, and 40+ more agents.

## Features

- DataPrime and PromQL at the terminal-Coralogix's proprietary query languages work end-to-end without leaving the shell.
- 28 commands across 9 domains-from querying signals to managing IAM, notifications, TCO, and archiving-all in one binary.
- Multi-profile fan-out with `-p prod-eu -p prod-us <command>`-run one command across multiple accounts or regions in a single invocation, with rows tagged by profile.
- `agents` output format-token-efficient JSON that auto-spills to a temp file once the serialized payload exceeds 100 KiB, so AI agents get a path instead of a flooded context window.
- `cx schema`-outputs the full command tree as structured JSON, purpose-built for agent discovery with no help-text parsing required.
- Semantic field search-find the right log or span field by describing it in natural language.
- Bundled skills for Claude Code, Cursor, Codex, OpenCode, and 40+ more agents, distributed via `npx skills add`.

## Install cx for AI agents

Most macOS users should use Homebrew. If you are not on macOS, use the cross-platform install script.

### 1. Install the CLI

```bash
curl -fsSL https://get.coralogix.dev/cli | sh
```

#### Recommended for macOS: Homebrew

```bash
brew install coralogix/tap/cx
```

### 2. Install agent skills

Skills teach your AI agent how to use `cx` for observability investigations.

```bash
npx skills add coralogix/cx-cli
```

### 3. Optional: install shell autocomplete

Most macOS users use `zsh`:

```bash
cx completions install zsh
```

For other shells, see [Shell completions](#shell-completions). For Cargo, pre-built binaries, Nix, and source builds, see [Installation reference](#installation-reference).

## Quick start

Follow these steps to go from a fresh install to a working query.

1. Create a profile. `cx profiles add <name>` opens an interactive prompt for region, credentials, and default output format:

    ```bash
    cx profiles add <name>
    ```

2. Query logs. The positional argument is a DataPrime query:

    ```bash
    cx logs 'filter $m.severity == ERROR'
    ```

3. Query metrics. `cx metrics query` takes a PromQL expression:

    ```bash
    cx metrics query 'rate(http_requests_total[5m])'
    ```

4. Search distributed spans. The positional argument is a DataPrime filter; `source spans` is prepended automatically:

    ```bash
    cx spans "filter \$l.serviceName == 'checkout'" --start now-2h --limit 50
    ```

5. List dashboards to confirm the API is reachable:

    ```bash
    cx dashboards catalog
    ```

6. Try semantic search to find dashboards or queries:

    ```bash
    # Find dashboards about error rates
    cx dashboards search "error rate monitoring"

    # Search dashboard query content
    cx dashboards query-search --description "http status 500"

    # Find queries using a specific field
    cx dashboards query-search --field '$d.http.status_code'
    ```

Run `cx <command> --help` for full syntax and examples on any command.

<details open markdown="1">
<summary><strong>Commands</strong></summary>

Commands are grouped by domain. Run `cx --help` for the full organized listing, or `cx schema` for a machine-readable JSON tree.

**Query**

| Command | Purpose |
|---|---|
| `cx logs` | Query logs using DataPrime |
| `cx spans` | Query distributed spans |
| `cx metrics` | Query metrics using PromQL: `query`, `query-range`, `search`, `get-labels` |
| `cx dataprime` | DataPrime language reference and raw queries: `list`, `show`, `query` |
| `cx search-fields` | Find log or span fields by description (default) or by value content (`-s value`) |

**Observe**

| Command | Purpose |
|---|---|
| `cx dashboards` | Manage or search dashboards and folders |
| `cx views` | Manage saved views and view folders |
| `cx slos` | Manage SLO definitions |

**Detect & Respond**

| Command | Purpose |
|---|---|
| `cx alerts` | Manage alerts: `list`, `get`, `create`, `delete`, `enable`, `disable`, `events`, `event-stats`, `suppression-rules` |
| `cx incidents` | Manage and triage incidents |

**Notifications**

| Command | Purpose |
|---|---|
| `cx notifications` | Manage connectors, routers, presets, and notification testing |
| `cx webhooks` | Manage outgoing webhooks and automation actions |

**Data Pipeline**

| Command | Purpose |
|---|---|
| `cx parsing-rules` | Manage log parsing rules |
| `cx enrichments` | Manage enrichment rules and custom enrichment tables |
| `cx e2m` | Manage Events2Metrics definitions |
| `cx recording-rules` | Manage Prometheus recording rule groups |

**Cost & Storage**

| Command | Purpose |
|---|---|
| `cx usage` | View data usage and consumption metrics |
| `cx tco` | Manage TCO policies and settings |
| `cx retentions` | Manage data retention settings |
| `cx archive` (risky) | Manage data archive storage configuration |

**Integrations**

| Command | Purpose |
|---|---|
| `cx integrations` | Manage integrations, extensions, and contextual data |

**Access**

| Command | Purpose |
|---|---|
| `cx iam` (risky) | Manage API keys, roles, scopes, users, groups, and IP access |

**Agent & Local**

| Command | Purpose |
|---|---|
| `cx schema` | Output the full command tree as JSON for agent consumption |
| `cx profiles` | Manage profiles: `list`, `add`, `delete`, `set-default` |
| `cx completions` | Shell tab-completion: `install`, `refresh`, `generate` |
| `cx cleanup` | Remove `cx_results*` temp files older than 30 minutes |

<details markdown="1">
<summary>Global options</summary>

```
-p, --profile <PROFILE>      Profile to use. Repeat to fan out across multiple profiles.
    --api-key <API_KEY>      Override the profile API key
    --region <REGION>        Override the profile region
-o, --output <FORMAT>        text | json | agents (default: text)
    --yes                    Skip confirmation prompts for destructive operations
```

</details>

</details>

## Configuration

Configuration lives in `~/.cx/`:

```
~/.cx/
  config.toml              # Global settings
  profiles/
    default.toml           # Credentials and region per profile
```

Credentials (API keys or OAuth tokens) are stored either inline in the profile TOML with `0600` permissions (`credential_storage = "file"`, the default) or in the OS keyring (`credential_storage = "os_store"` - macOS Keychain, Windows Credential Manager, or D-Bus Secret Service on Linux). `cx profiles add <name>` prompts for the choice. On Linux, keyring support requires a glibc build; the default install script and release binaries use musl, which has no keyring backend, so `os_store` is unavailable there.

Environment variables override profile settings: `CX_PROFILE`, `CX_API_KEY`, and `CX_REGION`.

See [docs/configuration.md](docs/configuration.md) for the full reference.

## Output formats

Choose an output format with `-o` or by setting the profile default.

- `text`-human-readable tables with color. Default.
- `json`-raw, pretty-printed API responses for scripting.
- `agents`-token-efficient format for AI agents. Large responses automatically spill to a temporary file and the path is returned.

See [docs/agents-output.md](docs/agents-output.md) for the `agents` format specification.

## AI agent skills

`cx` ships a companion skill bundle for Claude Code, Cursor, Codex, OpenCode, and [40+ other agents](https://github.com/vercel-labs/skills#supported-agents). The skills teach your agent how to investigate issues by querying Coralogix-without memorizing DataPrime syntax or API endpoints.

The install flow above installs all skills. Use these variants if you want to customize where or what you install.

Install selected skills:

```bash
npx skills add coralogix/cx-cli --skill query-logs --skill dataprime
```

Install globally for all projects:

```bash
npx skills add coralogix/cx-cli -g
```

Available skills: `cx-query-logs`, `cx-query-spans`, `cx-metrics-query`, `cx-alerts`, `cx-dataprime`, `cx-rum`, `cx-telemetry-querying`, `cx-dashboards`, `cx-cost-optimization`, `cx-incident-management`, `cx-data-pipeline`, `cx-platform-admin`, `cx-observability-setup`. See [skills/README.md](https://github.com/coralogix/cx-cli/blob/master/skills/README.md) for per-skill usage.

## Multi-profile fan-out

Repeat `-p` to run a command across multiple profiles in parallel. Results are merged and tagged with the profile name:

```bash
cx -p prod-eu -p prod-us logs 'filter $m.severity == ERROR'
```

See [docs/multi-profile.md](docs/multi-profile.md) for more examples.

## Installation reference

### Pin a CLI version

```bash
CX_VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/coralogix/cx-cli/master/install.sh | sh
```

### Cargo

```bash
cargo install coralogix-cli
```

### Pre-built binaries

Download the latest release for your platform from [GitHub Releases](https://github.com/coralogix/cx-cli/releases).

<details markdown="1">
<summary>Nix</summary>

```bash
nix run    github:coralogix/cx-cli -- --help     # try without installing
nix profile install github:coralogix/cx-cli      # install into your profile
```

The flake exposes both the `cx` binary and the agent skill bundle:

```nix
{
  inputs.cx-cli.url = "github:coralogix/cx-cli";

  outputs = { self, nixpkgs, cx-cli, ... }: {
    # cx-cli.packages.${system}.default -> the `cx` binary
    # cx-cli.packages.${system}.skills  -> store path with all cx-* skills
  };
}
```

#### Home Manager

Symlink each skill into `~/.claude/skills/` (adjust the target path for other agents):

```nix
# home.nix
{ inputs, pkgs, lib, ... }:
let
  skills = inputs.cx-cli.packages.${pkgs.system}.skills;
in {
  home.packages = [ inputs.cx-cli.packages.${pkgs.system}.default ];

  home.file = lib.mapAttrs'
    (name: _: lib.nameValuePair ".claude/skills/${name}" { source = "${skills}/${name}"; })
    (lib.filterAttrs (_: t: t == "directory") (builtins.readDir skills));
}
```

</details>

<details markdown="1">
<summary>Build from source</summary>

```bash
cargo build --release
cp target/release/cx /usr/local/bin/
```

</details>

## Shell completions

`cx` supports tab-completion for all commands, flags, subcommands, and profile names.

### Managed install (recommended)

Let `cx` install and track a completion script for you. It writes to a standard user-writable location and records the path so `cx completions refresh` can update it later:

```bash
cx completions install zsh
cx completions install bash
cx completions install fish
```

Default paths used by each shell:

| Shell | Default path |
|---|---|
| zsh | `~/.zfunc/_cx` |
| bash | `~/.local/share/bash-completion/completions/cx` |
| fish | `~/.config/fish/completions/cx.fish` |

After installing for **zsh**, add `~/.zfunc` to your `$fpath` if it isn't already there (the install command will tell you):

```bash
# Add to ~/.zshrc:
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

### Refreshing after profile changes

When you add or delete a profile, `cx` will remind you to refresh if you have managed completions. You can run it any time:

```bash
cx completions refresh
```

Only files previously installed by `cx completions install` are updated.

### Manual install

To generate a script yourself and pipe it anywhere:

```bash
cx completions generate zsh > ~/.zfunc/_cx
cx completions generate bash > ~/.local/share/bash-completion/completions/cx
cx completions generate fish > ~/.config/fish/completions/cx.fish
```

### Dynamic completions (always-fresh profile names)

For profile names to update automatically on every Tab press without running `refresh`, source completions dynamically on each shell start. This calls back into `cx` at completion time:

**zsh** - add to `~/.zshrc`:

```bash
source <(COMPLETE=zsh cx)
```

**bash** - add to `~/.bashrc`:

```bash
source <(COMPLETE=bash cx)
```

**fish** - add to `~/.config/fish/config.fish`:

```fish
COMPLETE=fish cx | source
```

<details markdown="1">
<summary><strong>Migrating from cxctl</strong></summary>

`cx` replaces the older Scala-based `cxctl`. If you are looking for documentation on the legacy tool, see the [Coralogix CLI (legacy) docs](https://coralogix.com/docs/developer-portal/infrastructure-as-code/cli/coralogix-cli/). `cx` does not currently cover all legacy surfaces, including LiveTail and account invite flows.

</details>

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for the ownership model,
PR review process, and step-by-step guides for adding commands and skills.

## Further reading

- [Configuration](docs/configuration.md)
- [Agents output format](docs/agents-output.md)
- [Multi-profile fan-out](docs/multi-profile.md)
- [Time syntax](docs/time-syntax.md)
- [Architecture](docs/architecture.md)
- [Development guide](docs/development.md)

## License

Apache-2.0
