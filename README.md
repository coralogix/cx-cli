# cx

[![CI](https://github.com/coralogix/cx-cli/actions/workflows/build.yml/badge.svg)](https://github.com/coralogix/cx-cli/actions/workflows/build.yml)
[![Crates.io](https://img.shields.io/crates/v/coralogix-cli)](https://crates.io/crates/coralogix-cli)
[![Homebrew](https://img.shields.io/homebrew/v/cx)](https://formulae.brew.sh/formula/cx)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

> **Coralogix on the command line.** `cx` gives you — and your AI agents — direct access to
> the full Coralogix platform from the terminal: query any signal, manage every resource,
> and wire Coralogix into automated workflows without leaving the shell.

```console
# Set up a profile (interactive)
$ cx profiles add
Profile name: prod-eu
Region: eu2
API key: ********

# Query error logs from the last hour
$ cx logs 'filter $m.severity == "ERROR"' --start now-1h --limit 5
┌────────────────────────┬──────────┬──────────────────────────────────────────┐
│ timestamp              │ severity │ message                                  │
├────────────────────────┼──────────┼──────────────────────────────────────────┤
│ 2026-05-05T09:14:02Z   │ ERROR    │ connection timeout to payments-svc       │
│ 2026-05-05T09:13:58Z   │ ERROR    │ failed to decode protobuf payload        │
└────────────────────────┴──────────┴──────────────────────────────────────────┘

# Fan out across multiple profiles in one command
$ cx -p prod-eu -p prod-us logs 'filter $m.severity == "CRITICAL"' --start now-15m

# Dump the command tree for AI agent discovery
$ cx schema | head -3
{"commands":[{"name":"logs","description":"Query logs using DataPrime syntax",...
```

## Why cx?

- **27 commands, one binary** — from querying signals to managing IAM, notifications, TCO, and archiving, all domains live in a single statically-linked executable.
- **Multi-profile fan-out** — repeat `-p` to run a command across multiple accounts or regions in parallel, with results merged and tagged by profile.
- **Built for AI agents** — `cx schema` dumps the full command tree as JSON for agent self-discovery; the `agents` output format is token-efficient and auto-spills large results to a temp file.
- **Bundled agent skills** — install ready-made skills for Claude Code, Cursor, Codex, and 40+ agents with `npx skills add coralogix/cx-cli`.
- **DataPrime and PromQL native** — Coralogix's query languages work end-to-end without leaving the shell, including semantic field search.

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
cargo install coralogix-cli
```

### Pre-built binaries

Download the latest release for your platform from [GitHub Releases](https://github.com/coralogix/cx-cli/releases).

<details>
<summary>Nix</summary>

```bash
nix run    github:coralogix/cx-cli -- --help     # try without installing
nix profile install github:coralogix/cx-cli      # install into your profile
```

Consume from another flake — both the `cx` binary and the agent skill bundle are exposed as outputs:

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

<details>
<summary>Build from source</summary>

```bash
cargo build --release
cp target/release/cx /usr/local/bin/
```

</details>

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

<details open>
<summary><strong>Commands</strong></summary>

Commands are grouped by domain. Run `cx --help` for the full organized listing, or `cx schema` for a machine-readable JSON tree.

**Query**

| Command | Purpose |
|---|---|
| `cx logs` | Query logs using DataPrime |
| `cx spans` | Query distributed spans |
| `cx metrics` | Query metrics using PromQL: `query`, `query-range`, `search`, `get-labels` |
| `cx dataprime` | DataPrime language reference and raw queries: `list`, `show`, `query` |
| `cx search-fields` | Find log or span fields by natural-language description |

**Observe**

| Command | Purpose |
|---|---|
| `cx dashboards` | Manage dashboards and folders |
| `cx views` | Manage saved views and view folders |
| `cx slos` | Manage SLO definitions |

**Detect & Respond**

| Command | Purpose |
|---|---|
| `cx alerts` | Manage alerts: `list`, `get`, `create`, `enable`, `disable`, `events`, `event-stats`, `suppression-rules` |
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
| `cx quotas` | Manage quota rules |
| `cx archive` (risky) | Manage data archive storage configuration |

**Integrations**

| Command | Purpose |
|---|---|
| `cx integrations` | Manage integrations, extensions, and contextual data |

**Access**

| Command | Purpose |
|---|---|
| `cx iam` (risky) | Manage API keys, roles, scopes, users, groups, SAML, and IP access |

**Agent & Local**

| Command | Purpose |
|---|---|
| `cx schema` | Output the full command tree as JSON for agent consumption |
| `cx profiles` | Manage profiles: `list`, `add`, `delete`, `set-default` |
| `cx completions` | Shell tab-completion: `install`, `refresh`, `generate` |
| `cx cleanup` | Remove `cx_results*` temp files older than 30 minutes |

<details>
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

Credentials are stored in the OS keyring on macOS (Keychain) and Windows (Credential Manager). On Linux, keyring support (Secret Service) requires a glibc build; the default install script and release binaries use musl, which has no keyring backend-credentials fall back to file storage. If you need keyring support on Linux, build from source with a glibc toolchain.

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

Available skills: `cx-query-logs`, `cx-query-spans`, `cx-metrics-query`, `cx-alerts`, `cx-dataprime`, `cx-rum`, `cx-telemetry-querying`, `cx-create-dashboard`, `cx-cost-optimization`, `cx-incident-management`, `cx-data-pipeline`, `cx-platform-admin`, `cx-observability-setup`. See [skills/README.md](skills/README.md) for per-skill usage.

## Multi-profile fan-out

Repeat `-p` to run a command across multiple profiles in parallel. Results are merged and tagged with the profile name:

```bash
cx -p prod-eu -p prod-us logs 'filter $m.severity == "ERROR"'
```

See [docs/multi-profile.md](docs/multi-profile.md) for more examples.

<details>
<summary><strong>Shell completions</strong></summary>

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

</details>

<details>
<summary><strong>Migrating from cxctl</strong></summary>

`cx` replaces the older Scala-based `cxctl`. If you are looking for documentation on the legacy tool, see the [Coralogix CLI (legacy) docs](https://coralogix.com/docs/developer-portal/infrastructure-as-code/cli/coralogix-cli/). `cx` does not currently cover all legacy surfaces, including LiveTail and account invite flows.

</details>

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for the ownership model,
PR review process, and step-by-step guides for adding commands and skills.

## Documentation

- [Configuration](docs/configuration.md)
- [Agents output format](docs/agents-output.md)
- [Multi-profile fan-out](docs/multi-profile.md)
- [Time syntax](docs/time-syntax.md)
- [Architecture](docs/architecture.md)
- [Development guide](docs/development.md)

## License

[Apache-2.0](LICENSE)
