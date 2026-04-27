# Configuration

`cx` stores all configuration under `~/.cx/`.

## Directory Structure

```
~/.cx/
  config.toml              # Global settings
  profiles/
    default.toml           # Default profile
    prod.toml              # Named profile
    staging.toml           # Named profile
```

## Quick Start

Run `cx profiles add` to create or update the default profile.  OAuth (browser login) is selected by default and is the recommended option — it opens your browser, captures the callback automatically, and stores tokens securely in the OS keyring.

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

Profile 'default' saved to /Users/you/.cx
Credentials stored in OS credential store (OAuth tokens)
```

To use a plain API key instead, select `API key (paste manually)` at the first prompt. The API key must be a **Team Key** or a **Personal Key** — see [API Key](#api-key) below for where to generate one. Send-Your-Data / ingress keys will not work for querying.

## Authentication Methods

### OAuth (default)

OAuth uses the standard browser-based Authorization Code + PKCE flow.

- Tokens (`access_token`, `refresh_token`, `id_token`) are stored in the OS keyring (macOS Keychain / Windows Credential Manager / libsecret) and are **never written to the profile TOML**.
- The access token is silently refreshed on each `cx` invocation when it is within 30 seconds of expiry.  
- If the refresh token is also expired, `cx` exits with an actionable message:  
  `Run cx profiles add <name> to re-authenticate.`

#### Custom / non-standard environments

If your environment is not in the standard list, select `Custom (specify URL + client ID)` at the Region prompt:

```
Region: Custom (specify URL + client ID)
Base URL (e.g. https://api.myenv.coralogix.com): https://api.myenv.example.com
OAuth client ID: abc123-my-client
```

The base URL is used both as the API endpoint and for OpenID Connect discovery (`{base_url}/oauth/.well-known/openid-configuration`). The client ID is stored in the profile TOML (`oauth_client_id`) since there is no built-in mapping for it.

### API Key

A static Coralogix API key. The key **must be one of the following types** — `cx` uses it as a Bearer token when calling the query APIs, so ingress ("Send-Your-Data") keys will not work:

- **Team Key** — generated in the Coralogix UI under *Data Flow → API Keys → Team Keys*. Scoped to a team; typical choice for shared/CI usage.
- **Personal Key** — generated in the Coralogix UI under the user menu (top-right) → *Personal Keys*. Scoped to your user account.

The key can be stored either in the profile TOML file (permissions set to `0600` on Unix) or in the OS keyring.

## Global Config (`~/.cx/config.toml`)

| Key | Default | Description |
|-----|---------|-------------|
| `default_profile` | `"default"` | Profile used when `--profile` is not provided |
| `default_output_format` | `"text"` | Output format when `--output` is not provided (`text`, `json`, `agents`) |
| `max_dataprime_direct_output_size` | `102400` (100 KiB) | Max byte size for non-aggregated DataPrime results in `agents` mode before spilling to a temp file. Set to `-1` to disable |
| `temp_dir` | `"/tmp/"` | Directory for spilled result files |

Example:

```toml
default_profile = "default"
default_output_format = "text"
max_dataprime_direct_output_size = 102400
temp_dir = "/tmp/"
```

## Profile Files (`~/.cx/profiles/<name>.toml`)

Each profile stores credentials and endpoint configuration.  The sensitive
secrets (API key, OAuth tokens) live in the OS keyring when
`credential_storage = "os_store"` and are **not** written to the TOML.

### Common fields

| Key | Required | Description |
|-----|----------|-------------|
| `auth` | No | `"oauth"` or `"api_key"` (default `"api_key"` for backward compat) |
| `region` | Yes | Coralogix region identifier or a custom URL (see below) |
| `credential_storage` | No | `"file"` or `"os_store"` (default `"file"`) |
| `label` | No | Free-form label (e.g. `"production"`) |

### OAuth-specific fields

| Key | When present | Description |
|-----|-------------|-------------|
| `oauth_client_id` | Custom environments | OAuth client ID.  Omitted for known regions (hard-coded). |
| `oauth_base_url` | Rarely | Override the base URL for OpenID discovery.  Defaults to `region.api_endpoint()`. |

### Example — OAuth profile (known region)

```toml
auth = "oauth"
credential_storage = "os_store"
region = "eu2"
label = "production"
```

Tokens are in the OS keyring; nothing sensitive is in this file.

### Example — OAuth profile (custom environment)

```toml
auth = "oauth"
credential_storage = "os_store"
region = "https://api.myenv.example.com"
oauth_client_id = "abc123-my-client"
label = "custom-env"
```

### Example — API key profile (OS keyring)

```toml
auth = "api_key"
credential_storage = "os_store"
region = "eu1"
label = "production"
```

The API key is in the OS keyring under the service `cx-cli`, profile name as the account.

### Example — API key profile (file, legacy)

```toml
region = "eu1"
api_key = "cxp_your_api_key_here"
label = "production"
```

Legacy profiles without an `auth` field behave as `auth = "api_key"` automatically.

## Regions

| Region | Endpoint |
|--------|----------|
| `us1` | `https://api.us1.coralogix.com` |
| `us2` | `https://api.us2.coralogix.com` |
| `us3` | `https://api.us3.coralogix.com` |
| `eu1` | `https://api.eu1.coralogix.com` |
| `eu2` | `https://api.eu2.coralogix.com` |
| `ap1` | `https://api.ap1.coralogix.com` |
| `ap2` | `https://api.ap2.coralogix.com` |
| `ap3` | `https://api.ap3.coralogix.com` |

A fully-qualified HTTPS URL may be used as a region value for non-standard environments.

## Environment Variables

Environment variables override profile file values:

| Variable | Overrides |
|----------|-----------|
| `CX_PROFILE` | `--profile` flag / `default_profile` |
| `CX_API_KEY` | `api_key` in profile (also overrides OAuth — sets the bearer token directly) |
| `CX_REGION` | `region` in profile |

**Precedence order:** CLI flags > environment variables > profile file > global config defaults.

> **Note:** `CX_API_KEY` / `--api-key` always win, even for OAuth profiles.  This
> lets scripts and CI systems inject tokens directly without going through the
> browser login flow.

## OAuth Callback Ports

The local HTTP callback listener used during `cx profiles add` (OAuth path) binds one
port from the following fixed allow-list, chosen at random:

```
21783  24861  27654  31847  38129
```

Ensure at least one of these ports is available when running `cx profiles add`.
