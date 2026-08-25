# Advanced configuration

`cx` stores all configuration under `~/.cx/`.

## Directory structure

```
~/.cx/
  config.toml              # Global settings
  profiles/
    default.toml           # Default profile
    prod.toml              # Named profile
    staging.toml           # Named profile
```

`cx init` writes both for you. `config.toml` holds the settings that apply to
every profile, including [`allow_risky_commands`](#global-config-cxconfigtoml)
and [`olly_enabled`](#global-config-cxconfigtoml), which are on by default. You
can edit them there at any time. Each file under `profiles/` is one connection to
Coralogix: its region or endpoint, its credentials, and its default output
format. Add as many as you have teams (see
[Multi-profile fan-out](multi-profile.md)).

## Profiles

`cx init` creates your first profile as part of setup — see the
[Quick start](quickstart.md) for that walkthrough. This page covers what it wrote,
and everything `init` deliberately does not ask about.

`cx init` defaults these rather than prompting: profile name `default`, `file`
credential storage, no label, and `json` as the profile's default output format.
Set `default_output_format` in the profile TOML (or pass `-o text`) if you would
rather read the output by eye than hand it to an agent.

It is idempotent — a bare re-run skips the profile step if a profile exists and
skips the skills step if the skills are installed. Any command that needs
credentials on a machine with no profile points you back at it:

```
No Coralogix profile is configured.
Run `cx init` to set up a profile and get started.
```

### Managing profiles

| Command | Purpose |
|---|---|
| `cx profiles list` | List every configured profile and show which one is the default. |
| `cx profiles add [NAME]` | Create or reconfigure a profile (see below). |
| `cx profiles set-default <NAME>` | Make an existing profile the default, so commands without `-p` use it. |
| `cx profiles delete <NAME>` | Delete a profile and its stored credentials. Asks for confirmation; `-f`/`--force` skips it. |

Naming a profile that doesn't exist is an error, not a fall back to the default:

```
Profile 'prod-us' not found. Run `cx profiles add` to set it up.
```

### Adding and reconfiguring profiles

`cx profiles add <name>` is the full-control path — use it for every profile after
the first, and for anything `init` doesn't ask about:

```sh
cx profiles add <name>
```

At the prompts, in order:

1. **Profile name** — asked only when you don't pass one. The first profile on the machine is pre-filled as `default`.
2. **Reconfigure?** — if a profile of that name already exists, `cx` asks `Reconfigure profile '<name>'?` before touching it. `--force` skips the confirmation.
3. **Authentication method** — `OAuth (browser login)` (recommended) opens your browser and stores tokens securely. Select `API key (paste manually)` to use a static key instead.
4. **Region / Coralogix URL** — your Coralogix region (e.g. `eu2`, `us1`), or paste your Coralogix URL. See [Regions](#regions) for the full list.
5. **Label** — an optional tag to group or identify profiles (e.g. `production`).
6. **Credential storage** — on the OAuth path this reads `Where should OAuth tokens be stored?`. `file` (default) saves credentials in the profile TOML with `0600` permissions; `os-store` uses the OS keyring (macOS Keychain, Windows Credential Manager, or D-Bus Secret Service on Linux).
7. **Default output format** — `text`, `json`, or `toon`.
8. **Set as default?** — asked for every profile *after* the first (`Set '<name>' as the default profile?`). The first profile you create always becomes the default. `--set-default` answers this up front.

Creating the first profile on a machine also writes the two global settings,
`allow_risky_commands` and `olly_enabled`. Both are on from the start, so Olly
and the full command set are available with no extra setup. Pass `--disable-olly`
to create the first profile with Olly switched off, and change either later in
[`config.toml`](#global-config-cxconfigtoml).

To re-authenticate an existing OAuth profile, run `cx profiles add <name>` again
for that name.

If using an API key, it must be a [Team Key](https://coralogix.com/docs/user-guides/account-management/api-keys/api-keys/#team-keys) or [Personal Key](https://coralogix.com/docs/user-guides/account-management/api-keys/api-keys/#personal-keys). [Send-Your-Data](https://coralogix.com/docs/user-guides/account-management/api-keys/send-your-data-api-key/) / ingress keys will not work.

### Non-interactive setup (CI, containers, coding agents)

The prompt-free `cx init` one-liner lives in the
[Quick start](quickstart.md#2-run-the-setup). Two behaviours are worth knowing
when you script it:

- Env-sourced values (`CX_API_KEY`, `CX_REGION`, `CX_PROFILE`) do not count as
  "flags typed on the command line", so they never break a bare re-run.
- Passing profile flags when a profile already exists fails rather than reporting
  a misleading success — add `--force` to reconfigure it.

`cx profiles add` takes the same treatment for additional profiles. Any value
supplied by flag or environment variable is never prompted for, and a run that
supplies both credentials and a region/URL prompts for nothing at all:

```sh
cx profiles add prod --region eu2                      # region answered, rest prompted
cx profiles add --oauth --region eu2                   # straight to browser login
cx profiles add --url https://myteam.app.eu2.coralogix.com --api-key $KEY
CX_API_KEY=$KEY cx profiles add --region us1 --force   # non-interactive overwrite
```

The positional `[NAME]` is prompted for when you omit it — but **only on a
terminal**. Non-interactively an omitted name becomes `default`; it is the one
value `cx` fills in for you without asking, so name the profile explicitly in
scripts unless you mean to write the default one.

| Flag | Purpose |
|---|---|
| `--name <NAME>` | Profile name, as an alternative to the positional argument. Called `--name` rather than `--profile` to stay clear of the global `--profile` selector. |
| `--url <URL>` | Derive the region from a Coralogix URL (your browser URL works). A URL `cx` doesn't recognise is used as a custom API endpoint — see [Custom or non-standard environments](#custom-or-non-standard-environments-byoc-private-link). Alternative to `--region`. |
| `--region <REGION>` | Region short-name (`us1`, `us2`, `us3`, `eu1`, `eu2`, `ap1`, `ap2`, `ap3`). |
| `--api-key <KEY>` | Team Key or Personal Key. Also read from `CX_API_KEY`. |
| `--oauth` | Use browser login and skip the auth-method prompt. Takes precedence over `--api-key`. Prints the sign-in URL, so it works without a terminal too (which then requires `--url` or `--region`). |
| `--force` | Overwrite an existing profile without prompting. |
| `--set-default` | Make this the default profile without prompting. |
| `--disable-olly` | Create the first profile with the Olly AI assistant switched off. Only meaningful for the first profile, since that is when `olly_enabled` is written. No prompt either way. |
| `-h` / `--help` | `--help` prints the full reference above; `-h` prints a short summary. |

Without a terminal, a missing required value is an error rather than a prompt, and
an existing profile is only overwritten with `--force`.

For environments with no profile file at all, see
[env-only mode](#environment-variables).

## Authentication methods

### OAuth (default)

OAuth uses the standard browser-based Authorization Code + PKCE flow.

- Tokens (`access_token`, `refresh_token`, `id_token`) are persisted using the chosen `credential_storage` backend - either inline in the profile TOML (`file`, the default) or in the OS keyring (`os_store`).
- The access token is silently refreshed on each `cx` invocation when it is within 30 seconds of expiry. The refreshed token set is written back to the same backend.
- If the refresh token is also expired, `cx` exits with an actionable message:

  ```
  Run cx profiles add <name> to re-authenticate.
  ```

#### Custom or non-standard environments (BYOC / private-link)

If your environment is not in the standard region list, choose `Custom endpoint (BYOC / private link)` at the region prompt and type the endpoint:

```
Region / Coralogix URL: Custom endpoint (BYOC / private link)
Base URL (e.g. https://api.myenv.coralogix.com): https://api.myenv.example.com
OAuth client ID: abc123-my-client
```

You usually get here without choosing it deliberately: paste any URL `cx` can't
map to a known region and the picker filters down to this option, keeping the URL
you already typed rather than asking again.

```
Region / Coralogix URL: Custom endpoint (BYOC / private link)
Couldn't map that URL to a known region - using it as a custom endpoint: https://api.myenv.example.com
OAuth client ID: abc123-my-client
```

The base URL is used both as the API endpoint and for OpenID Connect discovery (`{base_url}/oauth/.well-known/openid-configuration`). The client ID is stored in the profile TOML (`oauth_client_id`) since there is no built-in mapping for it.

Two things to watch when you paste rather than type:

- **Paste the API base URL, not a page you were looking at.** `cx` only trims a
  trailing slash and adds `https://` when the scheme is missing — it does **not**
  strip a path, query, or fragment. Pasting a console deep link such as
  `http://myenv.example.com:4200/#/olly` stores exactly that as the endpoint, and
  every later request is built on top of it. Trim it back to scheme, host, and
  port.
- **`http://` is accepted**, which is what makes a local or private-link
  deployment on a plain-HTTP address work. Nothing warns you, so double-check the
  scheme is the one you meant.

For a `Custom` region where `/identity/whoami` can't resolve a console link, set
`console_url` in the profile TOML by hand — see [Console links](#console-links).

#### Don't know your region? Paste your URL

The region prompt is a single searchable list — `Region / Coralogix URL:` — with
one row per region, labelled with its app domain (`(eu2) app.eu2.coralogix.com`),
plus a `Custom endpoint (BYOC / private link)` row at the bottom. `eu2` is
highlighted when the prompt opens.

You do not have to know your region. Paste the URL you use in the browser
(e.g. `https://myteam.app.eu2.coralogix.com`) into the prompt and the list filters
down to the one region it belongs to — press Enter to take it. `cx` recognises the
Coralogix app and API domains:

| URL you paste | Derived region |
|---|---|
| `https://<team>.app.coralogix.us` | `us1` |
| `https://<team>.app.cx498.coralogix.com` | `us2` |
| `https://<team>.app.us3.coralogix.com` | `us3` |
| `https://<team>.coralogix.com` | `eu1` |
| `https://<team>.app.eu2.coralogix.com` | `eu2` |
| `https://<team>.app.coralogix.in` | `ap1` |
| `https://<team>.app.coralogixsg.com` | `ap2` |
| `https://<team>.app.ap3.coralogix.com` | `ap3` |

The scheme is optional and a trailing path or query string is ignored, so
`myteam.app.eu2.coralogix.com` and
`https://myteam.app.eu2.coralogix.com/logs?query=foo` both work. API hosts
(`https://api.eu2.coralogix.com`) are recognised too.

If you would rather confirm it yourself before running anything, the
[Coralogix domain](https://coralogix.com/docs/user-guides/account-management/account-settings/coralogix-domain/) page lists every domain against its region and team
hostname.

A URL that doesn't match a known Coralogix domain — a bring-your-own-cloud
deployment, private link, or custom domain — filters down to
`Custom endpoint (BYOC / private link)` instead. Choosing it reuses the URL you
already typed rather than asking for it again, and `cx` reports what it did:

```
Couldn't map that URL to a known region - using it as a custom endpoint: https://api.myenv.example.com
```

Only a URL with no parseable host falls back to typing the endpoint manually.

The same applies outside the prompt: `cx init --url <URL>` and
`cx profiles add <name> --url <URL>` take a browser URL, and `--region`/`CX_REGION`
accept either a region short-name or a full endpoint URL.

### API key

A static Coralogix API key. The key **must be one of the following types** - `cx` uses it as a Bearer token when calling the query APIs, so ingress ("Send-Your-Data") keys will not work:

- **[Team Key](https://coralogix.com/docs/user-guides/account-management/api-keys/api-keys/#team-keys)** - generated in the Coralogix UI under *Data Flow → API Keys → Team Keys*. Scoped to a team; typical choice for shared/CI usage.
- **[Personal Key](https://coralogix.com/docs/user-guides/account-management/api-keys/api-keys/#personal-keys)** - generated in the Coralogix UI under the user menu (top-right) → *Personal Keys*. Scoped to your user account.

The key can be stored either in the profile TOML file (permissions set to `0600` on Unix) or in the OS keyring.

## OS keyring backends

Credentials stored in the OS keyring use the following backends:

| Platform | Backend |
|---|---|
| macOS | Keychain |
| Windows | Credential Manager |
| Linux (glibc) | D-Bus Secret Service (GNOME Keyring, KWallet) |
| Linux (musl) | No keyring backend - fall back to file-based storage |

The default install script and release binaries are built for musl on Linux, so keyring support is only available when you build from source against glibc. Script-installed Linux users must use file-based credential storage.

## Global config (`~/.cx/config.toml`)

| Key | Default | Description |
|---|---|---|
| `default_profile` | `"default"` | Profile used when `-p` is not provided |
| `default_output_format` | `"text"` | Output format when `-o` is not provided (`text`, `json`, `toon`; `agents` accepted as a deprecated alias) |
| `max_dataprime_direct_output_size` | `102400` (100 KiB) | Max byte size for non-aggregated DataPrime results in `toon` mode before spilling to a temp file. Set to `-1` to disable |
| `temp_dir` | `"/tmp/"` | Directory for spilled result files |
| `read_only` | `false` | Block all write operations globally (equivalent to always passing `--read-only`) |
| `no_console_link` | `false` | Suppress "View in Coralogix" console links globally (equivalent to always passing `--no-console-link`) |
| `allow_risky_commands` | `true` | Allow write operations under risky commands (`iam`, `archive`). Written when you create your first profile; never prompted. |
| `olly_enabled` | `true` | Enable the Olly AI assistant (`olly ask`). Written when you create your first profile; never prompted. Use `cx profiles add --disable-olly` to create it switched off. |

Example:

```toml
default_profile = "default"
default_output_format = "text"
max_dataprime_direct_output_size = 102400
temp_dir = "/tmp/"
read_only = false
no_console_link = false
allow_risky_commands = true
olly_enabled = true
```

## Output formats

Choose an output format with `-o` or by setting the profile default.

- `text`-human-readable tables with color. The global default.
- `json`-raw, pretty-printed API responses for scripting. Profiles created by `cx init` default to this.
- `toon`—token-efficient format for AI agents. Large responses automatically spill to a temporary file and the path is returned.

See [TOON output format](toon-output.md) for the `toon` format specification.

## Profile files (`~/.cx/profiles/<name>.toml`)

Each profile stores credentials and endpoint configuration. `credential_storage` selects where secrets live for both auth modes: with `"file"` (the default) the API key or OAuth token set is written inline in the TOML (`0600` perms on Unix); with `"os_store"` the secrets live in the OS keyring and the inline fields are absent.

### Common fields

| Key | Required | Description |
|---|---|---|
| `auth` | No | `"oauth"` or `"api_key"` (default `"api_key"` for backward compatibility) |
| `region` | Yes | Coralogix region identifier or a custom URL (see below) |
| `credential_storage` | No | `"file"` or `"os_store"` (default `"file"`) |
| `label` | No | Free-form label, for example `"production"` |
| `console_url` | No | Overrides the web console base URL used to build "View in Coralogix" links. If unset, `cx` resolves it automatically via `GET /identity/whoami` - see [Console links](#console-links). |

### OAuth-specific fields

| Key | When present | Description |
|---|---|---|
| `oauth_client_id` | Custom environments | OAuth client ID. Omitted for known regions (hard-coded in the binary). |
| `oauth_base_url` | Rarely | Overrides the base URL for OpenID discovery. Defaults to `region.api_endpoint()`. |
| `oauth_tokens` | `credential_storage = "file"` | Cached access/refresh/id tokens and expiry. Absent when tokens live in the OS keyring. |

### Example: OAuth profile (known region, file storage - default)

```toml
auth = "oauth"
credential_storage = "file"
region = "eu2"
label = "production"

[oauth_tokens]
access_token = "..."
refresh_token = "..."
expiry = 1700000000
```

The token set is rewritten in place each time `cx` refreshes. The file is created with `0600` permissions on Unix.

### Example: OAuth profile (known region, OS keyring)

```toml
auth = "oauth"
credential_storage = "os_store"
region = "eu2"
label = "production"
```

Tokens live in the OS keyring; nothing sensitive is in this file.

### Example: OAuth profile (custom environment)

```toml
auth = "oauth"
credential_storage = "file"
region = "https://api.myenv.example.com"
oauth_client_id = "abc123-my-client"
label = "custom-env"
```

### Example: API key profile (OS keyring)

```toml
auth = "api_key"
credential_storage = "os_store"
region = "eu1"
label = "production"
```

The API key is stored in the OS keyring under service `cx-cli`, profile name as the account.

### Example: API key profile (file, legacy)

```toml
region = "eu1"
api_key = "cxp_your_api_key_here"
label = "production"
```

Legacy profiles without an `auth` field behave as `auth = "api_key"` automatically.

## Regions

| Region | Endpoint |
|---|---|
| `us1` | `https://api.us1.coralogix.com` |
| `us2` | `https://api.us2.coralogix.com` |
| `us3` | `https://api.us3.coralogix.com` |
| `eu1` | `https://api.eu1.coralogix.com` |
| `eu2` | `https://api.eu2.coralogix.com` |
| `ap1` | `https://api.ap1.coralogix.com` |
| `ap2` | `https://api.ap2.coralogix.com` |
| `ap3` | `https://api.ap3.coralogix.com` |

A fully qualified HTTPS URL can be used as a region value for non-standard environments.

## Console links

After a successful command on entities with a corresponding web console page, `cx` prints a `View in Coralogix: <url>` line to stderr. The console base URL used to build these links is resolved in this order:

1. **`console_url`** in the profile TOML, if set - used as-is (see the field table above). No API call is made when this is set.
2. Otherwise, a **fresh cached `team_url`** in the profile TOML. The first time `cx` resolves a team's console URL (step 3) it writes the result back to the profile file as a machine-managed `cached_console_url` (with a `cached_console_url_at` timestamp), the same way OAuth tokens are cached there. Subsequent invocations reuse it for 7 days, so agents making many sequential calls don't pay a `GET /identity/whoami` round-trip on every command. Env-only invocations (`CX_API_KEY` + `CX_REGION` with no profile file) have nowhere to cache and resolve live each process.
3. Otherwise, `cx` calls `GET /identity/whoami` and uses its `team_url` field verbatim, then caches it per step 2. This is the default on a cold cache - most teams don't need to configure anything to get console links.
4. **No link is printed** if `/identity/whoami` fails, or returns no usable `team_url`.

A failed or unusable `/identity/whoami` response in step 3 never fails the command - it just means no console link, exactly like step 4. When no link can be resolved, `cx` stays silent: no `View in Coralogix` line on stderr, and the command otherwise succeeds normally.

Set `console_url` explicitly in the profile TOML to override this, e.g. for a `Custom` region running a self-hosted console where `/identity/whoami` isn't reachable.

### Disabling console links

The stderr `View in Coralogix` line can be suppressed entirely. This also skips the `/identity/whoami` lookup on a cold cache.

There are three ways to enable it, listed from narrowest to broadest scope:

| Method | Scope | Example |
|---|---|---|
| `--no-console-link` flag | Single invocation | `cx --no-console-link alerts list` |
| `CX_NO_CONSOLE_LINK` env var | Shell session / CI job | `export CX_NO_CONSOLE_LINK=true` |
| `no_console_link = true` in `~/.cx/config.toml` | All invocations | See global config table above |

## Environment variables

Environment variables override profile file values:

| Variable | Overrides |
|---|---|
| `CX_PROFILE` | `-p` flag / `default_profile` |
| `CX_API_KEY` | `api_key` in profile (also overrides OAuth - sets the bearer token directly) |
| `CX_REGION` | `region` in profile |
| `CX_READ_ONLY` | `read_only` in global config (accepts `1`, `true`, `yes`, `on`) |
| `CX_NO_CONSOLE_LINK` | `no_console_link` in global config (accepts `1`, `true`, `yes`, `on`) |
| `CX_TELEMETRY` | Set to `false`, `no`, `off`, or `0` to disable CLI request metadata |

**Precedence order:** CLI flags > environment variables > profile file > global config defaults.

> **Note:** `CX_API_KEY` / `--api-key` always win, even for OAuth profiles. This lets scripts and CI systems inject tokens directly without going through the browser login flow.

> **Env-only mode:** when no profile file exists on disk but both `CX_API_KEY` (or `--api-key`) and `CX_REGION` (or `--region`) are supplied, `cx` runs without a profile file. This is convenient for ephemeral environments (CI runners, containers, ad-hoc scripts) where running `cx profiles add <name>` first would be a paper-cut.

### Request metadata

Each authenticated Coralogix API request includes bounded `X-Cx-Cli-*` headers
for the current invocation: command path and family, output format,
authentication type, installed CX skills, selected and configured
profile counts, and write-operation and `--yes` flags.
`X-Cx-Cli-Metadata` also contains the same values as compact JSON.
`X-Cx-Cli-Installed-Skills` is a sorted JSON list of installed skill directory
names, without filesystem paths. It lists only the skills bundled with this
`cx` version that are found installed.
`X-Cx-Cli-Is-Agent` is `true` when the master agent-environment detector
matches, and `false` otherwise.

The installation identifier is a random UUID stored in `~/.cx/state.json`; it
identifies a CLI installation, not a person or API key. Request metadata never
includes API keys, profile names, command arguments, query text, raw error
messages, arbitrary environment variables, outcome, error details, HTTP
status, or duration.

Set `CX_TELEMETRY=false` to opt out of these headers entirely.

## Read-only mode

Read-only mode blocks all write operations (create, update, delete, enable, disable, etc.) while allowing reads (list, get, query, search). This is useful for giving agents or automation safe, query-only access to your Coralogix data.

There are three ways to enable it, listed from narrowest to broadest scope:

| Method | Scope | Example |
|---|---|---|
| `--read-only` flag | Single invocation | `cx --read-only alerts list` |
| `CX_READ_ONLY` env var | Shell session / CI job | `export CX_READ_ONLY=true` |
| `read_only = true` in `~/.cx/config.toml` | All invocations | See global config table above |

When a write operation is attempted in read-only mode, `cx` exits with an error:

```
Error: Write operation 'create' is blocked in read-only mode
(--read-only flag, CX_READ_ONLY env var, or read_only = true in ~/.cx/config.toml).
```

Local commands (`profiles`, `cleanup`, `completions`) are exempt from read-only enforcement - they manage local configuration and never touch the Coralogix API.

The env var accepts `1`, `true`, `yes`, or `on` (case-insensitive).

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
Every release ships a SHA-256 checksum and a signature beside each artifact, so
this is the route to take when a security policy rules out piping a shell script
into `sh`. On Windows the artifact is `cx-<version>-x86_64-pc-windows-msvc.zip`:
unzip it and put `cx.exe` on your `PATH`.

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

### Profile-name completion

Profile names discovered in `~/.cx/profiles/*.toml` are offered as tab-completion candidates for the `-p`/`--profile` flag and for the `profiles add`, `profiles delete`, and `profiles set-default` subcommands.

When using static completions installed with `cx completions install`, profile names are captured at installation time. After adding or deleting a profile, `cx` will print a reminder to run `cx completions refresh`, which regenerates every file registered in `managed_completions`.

The `managed_completions` field in `~/.cx/config.toml` is updated automatically by `cx completions install` and is read by `cx completions refresh`. Only files recorded here are ever modified by `cx`; files installed by `cx completions generate ... > /path` are not tracked.

For profile names that are always resolved fresh without a manual refresh step, use [dynamic completions](#dynamic-completions-always-fresh-profile-names) instead.

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

## OAuth callback ports

The local HTTP callback listener used by the OAuth browser login (`cx init`, or `cx profiles add <name>` on the OAuth path) binds one port from the following fixed allow-list, chosen at random:

```
21783  24861  27654  31847  38129
```

Ensure at least one of these ports is available when signing in.
