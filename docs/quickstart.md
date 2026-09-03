# Quick start

From nothing installed to a working query. If you only read one page, read this one.

## Let your coding agent do it

Paste this into Claude Code, Cursor, Codex, or any other coding agent:

```text
Install and set up the Coralogix CLI by following https://coralogix.com/docs/cli .
My Coralogix URL is [paste your Coralogix URL]
```

Replace `[paste your Coralogix URL]` with the address you use to open Coralogix in
your browser.

Or, you can [Install manually](#install-manually).

## Install manually

### 1. Install the CLI

**macOS / Linux** - Homebrew is the recommended route on macOS:

```bash
brew install coralogix/tap/cx
```

Or use the cross-platform install script:

```bash
curl -fsSL https://get.coralogix.dev/cli | sh
```

If your security policy blocks piped shell scripts, use a signed release binary
instead - see [Installation reference](configuration.md#installation-reference).

**Windows** - download `cx-<version>-x86_64-pc-windows-msvc.zip` from
[GitHub Releases](https://github.com/coralogix/cx-cli/releases), unzip it, and put
`cx.exe` on your `PATH`. Every release ships a SHA-256 checksum and a signature
beside it. If you already have Rust, `cargo install coralogix-cli` works on every
platform.

### 2. Run the setup

One command walks through the whole thing. It signs you in through your browser,
so there is no key to paste:

```bash
cx init
```

What it asks you:

| Prompt | What it does |
|---|---|
| **Region / Coralogix URL** | **Don't know your region? You don't need to.** Paste the URL you use to reach Coralogix in the browser (e.g. `https://myteam.app.eu2.coralogix.com`) and `cx` works the region out for you. Pick it from the list instead if you do know it, or look yours up in the [Coralogix domain](https://coralogix.com/docs/user-guides/account-management/account-settings/coralogix-domain/) table. A URL `cx` doesn't recognise - a bring-your-own-cloud or private-link deployment - becomes a custom API endpoint instead of an error. |
| **Sign in** | Opens your browser for [OAuth login](https://coralogix.com/docs/user-guides/account-management/user-management/oauth/). Approve the scopes and choose which team the CLI may access, then come back to the terminal. |
| **Where to put the agent skills** | Local to this project (`./`) or global (`~/`). `cx` installs them for you and tells you how to update them later. |
| **Install shell completions?** | Turns on `<Tab>` completion for `cx`. Pick `zsh`, `bash`, or `fish`, or keep the default and skip it. **Other** lets you name a shell and an install path yourself. Skipped without asking when completions are already installed. |

Everything else is defaulted rather than asked: profile name `default`, `file`
credential storage, no label, and `json` as the profile's default output format
(pass `-o text` for a run you want to read by eye, or set `default_output_format`
in the profile). You also get [Olly](https://coralogix.com/docs/user-guides/olly/ask-olly/), Coralogix's AI assistant, and the
full command set including `iam` and `archive` writes, all available from the
start. To switch either off, set `olly_enabled` or `allow_risky_commands` to
`false` in `~/.cx/config.toml`, or create your first profile with
`cx profiles add --disable-olly`.

`cx init` is idempotent: on a machine that already has a profile it skips the
profile step, and on one that already has the skills it skips the install. Nothing
is re-prompted or overwritten. To reconfigure a profile later use
`cx profiles add --force`; to pull the latest skills use `cx skills install`.

For CI, containers, and coding agents, spell the whole thing out and the run is
prompt-free:

```bash
cx init --url https://myteam.app.eu2.coralogix.com --api-key $CX_API_KEY --global-skills
```

| Flag | Purpose |
|---|---|
| `--url <URL>` | Derive the region from a Coralogix URL. Unrecognised URLs are used as a custom API endpoint (BYOC / private link). |
| `--oauth` | Force browser login even when an API key is available. |
| `--global-skills` / `--local-skills` | Answer the skills-scope question up front. Without one of these and with no terminal, the skills step is skipped with a warning. |
| `--agent <name>` | Target specific agents instead of letting the installer auto-detect. Repeatable. |
| `--no-skills` | Skip the agent-skills step. Conflicts with `--global-skills`, `--local-skills`, and `--agent`. |
| `--install-completions <shell>` | Install completions for `zsh`, `bash`, or `fish` without prompting. Omit it and an interactive run asks; a run with no terminal skips the step. Ignored when completions are already installed. |

With no terminal and no API key, `cx init` fails immediately and names
`--api-key` rather than hanging on a prompt. A failed skills install never blocks
onboarding - it downgrades to a warning, because a working profile is already a
usable setup.

`cx profiles add` is still there for everything `init` deliberately doesn't ask
about - additional profiles, profile labels, API-key auth, OS-keyring credential
storage, and a non-default output format. See
[Advanced configuration](configuration.md).

### 3. Ask it something

The answer is the confirmation. If your agent comes back with your data, it is
connected:

```bash
cx logs 'source logs | limit 10'
cx schema        # discover every command as JSON
```

Any command that needs credentials on a machine with no profile tells you exactly
what to do next:

```
No Coralogix profile is configured.
Run `cx init` to set up a profile and get started.
```

### 4. Optional: add autocomplete for another shell

`cx init` offers this during setup, so most people are already done. Use this to
add a second shell, or if you skipped it there:

```bash
cx completions install zsh
```

For other shells, see [Shell completions](configuration.md#shell-completions). For Cargo,
pre-built binaries, Nix, and source builds, see
[Installation reference](configuration.md#installation-reference).

## First queries

Once `cx init` is done, these cover the shapes you'll use most.

1. Query logs. The positional argument is a DataPrime query:

    ```bash
    cx logs 'filter $m.severity == ERROR'
    ```

2. Query metrics. `cx metrics query` takes a PromQL expression:

    ```bash
    cx metrics query 'rate(http_requests_total[5m])'
    ```

3. Search distributed spans. The positional argument is a DataPrime filter; `source spans` is prepended automatically:

    ```bash
    cx spans "filter \$l.serviceName == 'checkout'" --start now-2h --limit 50
    ```

4. List dashboards to confirm the API is reachable:

    ```bash
    cx dashboards catalog
    ```

5. Try semantic search to find dashboards or queries:

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
| `cx docs` | Search and fetch official Coralogix product documentation: `search`, `fetch` |
| `cx search-fields` | Find log or span fields by description (default) or by value content (`-s value`) |

**Observe**

| Command         | Purpose                                                                |
|-----------------|------------------------------------------------------------------------|
| `cx dashboards` | Manage or search dashboards and folders                                |
| `cx views`      | Manage saved views and view folders                                    |
| `cx slos`       | Manage SLO definitions                                                 |
| `cx infra`      | Get infrastructure data: `types`, `list`, `health-history`, `raw-data` |
| `cx service-catalog` | Query service-catalog entities and their RED / health / saturation data |

**AI**

| Command | Purpose |
|---|---|
| `cx ai-center` (risky) | Manage AI Center applications, evaluations, policies, and pricing: `applications`, `evaluations`, `custom-evaluations`, `coverage`, `model-pricing` |

**Detect & Respond**

| Command | Purpose |
|---|---|
| `cx alerts` | Manage alerts: `list`, `get`, `create`, `delete`, `enable`, `disable`, `events`, `event-stats`, `suppression-rules` |
| `cx cases` | Manage and triage cases |

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

**Agent**

| Command | Purpose |
|---|---|
| `cx schema` | Output the full command tree as JSON for agent consumption |
| `cx olly` | Interact with the Olly AI assistant: `ask` |

**Local**

These need no API credentials and are exempt from the risky-command confirmation.

| Command | Purpose |
|---|---|
| `cx init` | One-step onboarding: configure a profile and install the agent skills |
| `cx profiles` | Manage profiles: `list`, `add`, `delete`, `set-default` |
| `cx skills` | Install or update the cx agent skills for coding agents: `install` |
| `cx completions` | Shell tab-completion: `install`, `refresh`, `generate` |
| `cx cleanup` | Remove `cx_results*` temp files older than 30 minutes |

<details markdown="1">
<summary>Global options</summary>

```
-p, --profile <PROFILE>      Profile to use. Repeat to fan out across multiple profiles.
    --api-key <API_KEY>      Override the profile API key
    --region <REGION>        Override the profile region
-o, --output <FORMAT>        text | json | toon (default: text)
    --yes                    Skip confirmation prompts for destructive operations
    --read-only              Block all write operations. Useful for safe agent access.
    --no-console-link        Suppress "View in Coralogix" console links
```

</details>

</details>
