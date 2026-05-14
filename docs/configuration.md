# Configuration

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

## Quick start

Run `cx profiles add` to create or update the default profile. OAuth (browser login) is selected by default and is the recommended option - it opens your browser, captures the callback automatically, and persists the resulting tokens.

```
$ cx profiles add
Configuring profile 'default'

Authentication method: OAuth (browser login)
Region: eu2
Label (e.g. 'prod'): production

Opening browser for authentication...
Waiting for browser callback...
Authorization code received, exchanging for tokens...
Login successful!

Where should OAuth tokens be stored? file
Profile 'default' saved to /Users/you/.cx
Credentials stored in profile file (OAuth tokens)
```

You will be asked where to store the credentials: `file` (default - saved inline in the profile TOML with `0600` permissions) or `os-store` (the OS credential store - macOS Keychain, Windows Credential Manager, etc.). Pick `os-store` if your threat model assumes other processes running as your user could read the profile file.

To use a plain API key instead, select `API key (paste manually)` at the first prompt. The API key must be a [Team Key](https://coralogix.com/docs/user-guides/account-management/api-keys/api-keys/#team-keys) or a [Personal Key](https://coralogix.com/docs/user-guides/account-management/api-keys/api-keys/#personal-keys) - see [API Key](#api-key) below for where to generate one. [Send-Your-Data](https://coralogix.com/docs/user-guides/account-management/api-keys/send-your-data-api-key/) / ingress keys will not work for querying.

## Authentication methods

### OAuth (default)

OAuth uses the standard browser-based Authorization Code + PKCE flow.

- Tokens (`access_token`, `refresh_token`, `id_token`) are persisted using the chosen `credential_storage` backend - either inline in the profile TOML (`file`, the default) or in the OS keyring (`os_store`).
- The access token is silently refreshed on each `cx` invocation when it is within 30 seconds of expiry. The refreshed token set is written back to the same backend.
- If the refresh token is also expired, `cx` exits with an actionable message:

  ```
  Run cx profiles add <name> to re-authenticate.
  ```

#### Custom or non-standard environments

If your environment is not in the standard region list, select `Custom (specify URL + client ID)` at the Region prompt:

```
Region: Custom (specify URL + client ID)
Base URL (e.g. https://api.myenv.coralogix.com): https://api.myenv.example.com
OAuth client ID: abc123-my-client
```

The base URL is used both as the API endpoint and for OpenID Connect discovery (`{base_url}/oauth/.well-known/openid-configuration`). The client ID is stored in the profile TOML (`oauth_client_id`) since there is no built-in mapping for it.

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
| `default_output_format` | `"text"` | Output format when `-o` is not provided (`text`, `json`, `agents`) |
| `max_dataprime_direct_output_size` | `102400` (100 KiB) | Max byte size for non-aggregated DataPrime results in `agents` mode before spilling to a temp file. Set to `-1` to disable |
| `temp_dir` | `"/tmp/"` | Directory for spilled result files |
| `read_only` | `false` | Block all write operations globally (equivalent to always passing `--read-only`) |
| `allow_risky_commands` | `true` | Allow write operations under risky commands (`iam`, `archive`) |
| `olly_enabled` | `true` | Enable the Olly AI assistant (`olly ask`) |

Example:

```toml
default_profile = "default"
default_output_format = "text"
max_dataprime_direct_output_size = 102400
temp_dir = "/tmp/"
read_only = false
allow_risky_commands = true
olly_enabled = true
```

## Profile files (`~/.cx/profiles/<name>.toml`)

Each profile stores credentials and endpoint configuration. `credential_storage` selects where secrets live for both auth modes: with `"file"` (the default) the API key or OAuth token set is written inline in the TOML (`0600` perms on Unix); with `"os_store"` the secrets live in the OS keyring and the inline fields are absent.

### Common fields

| Key | Required | Description |
|---|---|---|
| `auth` | No | `"oauth"` or `"api_key"` (default `"api_key"` for backward compatibility) |
| `region` | Yes | Coralogix region identifier or a custom URL (see below) |
| `credential_storage` | No | `"file"` or `"os_store"` (default `"file"`) |
| `label` | No | Free-form label, for example `"production"` |

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

## Environment variables

Environment variables override profile file values:

| Variable | Overrides |
|---|---|
| `CX_PROFILE` | `-p` flag / `default_profile` |
| `CX_API_KEY` | `api_key` in profile (also overrides OAuth - sets the bearer token directly) |
| `CX_REGION` | `region` in profile |
| `CX_READ_ONLY` | `read_only` in global config (accepts `1`, `true`, `yes`, `on`) |

**Precedence order:** CLI flags > environment variables > profile file > global config defaults.

> **Note:** `CX_API_KEY` / `--api-key` always win, even for OAuth profiles. This lets scripts and CI systems inject tokens directly without going through the browser login flow.

> **Env-only mode:** when no profile file exists on disk but both `CX_API_KEY` (or `--api-key`) and `CX_REGION` (or `--region`) are supplied, `cx` runs without a profile file. This is convenient for ephemeral environments (CI runners, containers, ad-hoc scripts) where running `cx profiles add` first would be a paper-cut.

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

## Shell completion and profiles

Profile names discovered in `~/.cx/profiles/*.toml` are offered as tab-completion candidates for the `-p`/`--profile` flag and for the `profiles add`, `profiles delete`, and `profiles set-default` subcommands.

When using static completions installed with `cx completions install`, profile names are captured at installation time. After adding or deleting a profile, `cx` will print a reminder to run `cx completions refresh`, which regenerates every file registered in `managed_completions`.

The `managed_completions` field in `~/.cx/config.toml` is updated automatically by `cx completions install` and is read by `cx completions refresh`. Only files recorded here are ever modified by `cx`; files installed by `cx completions generate ... > /path` are not tracked.

For profile names that are always resolved fresh without a manual refresh step, use the dynamic completion approach instead: `source <(COMPLETE=zsh cx)` (or the bash/fish equivalent). See the [Shell completions](../README.md#shell-completions) section in the README for full setup instructions.

## OAuth callback ports

The local HTTP callback listener used during `cx profiles add` (OAuth path) binds one port from the following fixed allow-list, chosen at random:

```
21783  24861  27654  31847  38129
```

Ensure at least one of these ports is available when running `cx profiles add`.
