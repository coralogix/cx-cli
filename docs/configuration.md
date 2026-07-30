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

Run `cx profiles add <name>` to create a new profile or update an existing one:

```sh
cx profiles add <name>
```

At the prompts:

1. **Authentication method** — `OAuth (browser login)` (recommended) opens your browser and stores tokens securely. Select `API key (paste manually)` to use a static key instead.
2. **Region** — your Coralogix region (e.g. `eu2`, `us1`). See [Regions](#regions) for the full list.
3. **Label** — an optional tag to group or identify profiles (e.g. `production`).
4. **Credential storage** — `file` (default) saves credentials in the profile TOML; `os-store` uses the OS keyring (macOS Keychain, Windows Credential Manager, or D-Bus Secret Service on Linux).

If using an API key, it must be a [Team Key](https://coralogix.com/docs/user-guides/account-management/api-keys/api-keys/#team-keys) or [Personal Key](https://coralogix.com/docs/user-guides/account-management/api-keys/api-keys/#personal-keys). [Send-Your-Data](https://coralogix.com/docs/user-guides/account-management/api-keys/send-your-data-api-key/) / ingress keys will not work.

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
| `console_url` | No | Overrides the web console base URL used to build "View in Coralogix" links (e.g. `https://acme.app.eu2.coralogix.com`). If unset, `cx` derives it from the region's console domain plus a team subdomain fetched via `GET /identity/whoami` - see [Console links](#console-links) for exactly how that lookup works. |

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

After a successful command on any of the entities below, `cx` prints a `View in Coralogix: <url>` line to stderr linking directly to the affected entity (or, for the "static page" groups further down, the relevant settings/list page) in the web console. The same URL is also embedded as a `consoleUrl` field directly in `-o json` / `-o agents` output, so agent/script consumers reading stdout get the link without having to parse stderr - the stderr line and the `consoleUrl` field are always emitted together and never disagree. On commands whose result is a list of rows rather than a single object (e.g. `dashboards check`'s validation issues, or any `list`/`search`), the link is the same static page for every row in that profile's chunk, so it's only attached to the first row rather than repeated on every one - a 200-item list shouldn't repeat an identical `consoleUrl` string 200 times in `-o agents` output. A failure to resolve a link never fails the command; it just means no `View in Coralogix` line is printed and no `consoleUrl` field is added.

> The rule for what gets a link is simply *"does a real console page exist for this?"* - not *"did this command create or update a specific entity."* A command can have no notion of a created/updated entity at all (e.g. `cx usage`, which is 100% read-only reporting, or `cx olly ask`, which starts a chat) and still earn a link, as long as a routed page for it exists in `coralogix/cx-web-workspace`. This applies just as much to *reads* as to writes: if a group's console page is confirmed to exist, every subcommand that surfaces data from that page - `list`, `get`, `search`, `test`, `settings`, etc. - prints the link, not only `create`/`update`/`delete`. Conversely, some commands (e.g. `cx actions`, `cx ai-center coverage`/`model-pricing`) do **not** get a link because no routed page could be confirmed for them - see the PR description of the change that introduced this table for the full per-subcommand research record.

The web console is a single-page app that routes client-side off a `#/` hash fragment, so every link below includes that prefix (e.g. `.../#/dashboards/<id>`, not `.../dashboards/<id>`) - a link missing it would just load the console's default screen instead of the intended entity:

Every link shape below was cross-checked against the console frontend's own routing source (`coralogix/cx-web-workspace`), not just public docs, so none of them are guesses.

### Per-entity links (path segment or query param)

These link directly to the specific entity that was just created/updated, either as a path segment or as a query parameter that makes the console auto-open/select that entity on load.

| Command | Link shape | Source |
|---|---|---|
| `dashboards create`/`replace`/`get`/`check <id>` | `{base}/#/dashboards/{id}` | Documented with an example at [Share Dashboard URLs](https://coralogix.com/docs/user-guides/custom-dashboards/tutorials/share-dashboard-content/); confirmed in frontend source (`libs/dashboards/_ui/src/lib/routing-utils.ts`'s `dashboardsEditUrl()` and the `:id` route under root `dashboards`). `check` only links when validating a stored dashboard by id (`cx dashboards check <id>`), not when checking `--from-file`, since there's no persisted entity to link to in that case |
| `alerts create`/`get`/`enable`/`disable` | `{base}/#/alerts/{id}` | Documented alert deep-link pattern (runbooks/webhooks reference `#/alerts/<id>`); confirmed in frontend source (`apps/web-app/src/app/alerts/alerts-routes.ts`, `:id` child route under `alerts`) |
| `views create`/`update` | `{base}/#/explore?viewId={id}` | `cx views` manages the same "saved view" entity as Explore's documented `viewId` deep-link parameter (both live under the `data-exploration/views` API) - see [Deep links and URL parameters](https://coralogix.com/docs/user-guides/data_exploration/deep-links/); confirmed in frontend source (`libs/explore/v2/src/lib/services/share-url.service.ts`'s `viewIdParam()`) |
| `cases get`, `cases` lifecycle mutations | `{base}/#/cases?id={id}` | No public doc names the exact query parameter, but it's confirmed directly in frontend source: `libs/cases/.../cases-query-params.constants.ts` defines `SELECTED_CASE_QUERY_PARAM = 'id'`, used by `insights-incidents-link.service.ts` to build case deep links as `/cases?id=<caseId>` |
| `e2m create`/`update` | `{base}/#/tco/metrics/{id}` | Confirmed as a real per-entity path-segment route in frontend source (Events2Metrics editor route takes `:metricId` under `tco/metrics`) |
| `slos create`/`update` | `{base}/#/slo/{id}/overview` | Confirmed as a real per-entity path-segment route in frontend source (SLO overview route takes `:sloId` under root `slo`) |
| `parsing-rules create`/`update` | `{base}/#/rules/group/{id}` | Confirmed as a real per-entity path-segment route in frontend source (rule group editor route takes `:themeId` under `rules/group`) |
| `alerts suppression-rules create`/`update` | `{base}/#/suppression-rules?edit={id}` | Confirmed in frontend source: suppression rules are a **top-level** route (not nested under `/alerts`); the editor component reads the rule id to open from an `edit` query param on load |
| `notifications connectors create`/`update`/`get` | `{base}/#/notification-center/connectors?id={id}` | Confirmed in frontend source: the connectors list component reads `route.snapshot.queryParams['id']` to auto-open that connector's editor on load |
| `notifications routers create`/`update` | `{base}/#/notification-center/routers?id={id}` | Confirmed in frontend source: the routers list component reads `route.snapshot.queryParams['id']` to auto-open that router's editor on load, mirroring the connectors pattern |
| `iam roles create`/`update` | `{base}/#/settings/roles?selectedRoleId={id}` | Confirmed in frontend source: the roles settings page reads `selectedRoleId` from query params to auto-select/open that role |
| `iam scopes create`/`update` | `{base}/#/settings/scopes?selectedScopeId={id}` | Confirmed in frontend source: the scopes settings page reads `selectedScopeId` from query params, mirroring the roles pattern |
| `iam groups create`/`update` | `{base}/#/settings/account/groups?selectedGroupId={id}` | Confirmed in frontend source: the account groups settings page reads `selectedGroupId` from query params, mirroring the roles/scopes pattern |

### Static, per-feature links (no per-entity ID)

Some entities live on a settings/list page rather than a per-instance route - there's no `:id` or `?id=` to fill in, just one fixed page per feature. For these, `cx` links to that static page after **every** subcommand in the group that touches it, reads included - editors for this kind of page are in-page dialogs with no id reflected in the URL, so a `list`/`get` is just as "on that page" as a `create`/`update`.

| Command | Link shape | Source |
|---|---|---|
| `usage` (all subcommands - fully read-only, no mutation to gate on) | `{base}/#/settings/datausage` | Confirmed as a real routed settings page in frontend source |
| `tco list`/`get`/`create`/`update`/`delete`/`reorder`/`test`/`settings get`/`settings update` | `{base}/#/tco-policies` | Confirmed as a real routed page in frontend source |
| `archive metrics get`/`create`/`update`/`enable`/`disable`/`validate`, `archive logs get`/`set` | `{base}/#/physical-locations` | Confirmed as a real routed page in frontend source; shared by both the metrics- and logs-archive subtrees, which configure the same underlying storage locations |
| `recording-rules list`/`get`/`create`/`update`/`delete` | `{base}/#/recording-rules` | Confirmed as a real routed page in frontend source |
| `enrichments list`/`add`/`remove`/`overwrite`/`limit`/`settings`, `enrichments custom list`/`get`/`search`/`create`/`update`/`delete` | `{base}/#/enrichments` | Confirmed as a real routed page in frontend source; shared by both enrichment rules and custom enrichment tables, which are tabs on the same page |
| `integrations list`/`get`/`definition`/`deployed`/`template`/`create`/`update`/`delete`/`test`, `integrations extensions list`/`get`/`deployed`/`deploy`/`update`/`undeploy`, `integrations contextual-data list`/`get`/`definition`/`test`/`create`/`update`/`delete` | `{base}/#/extensions/integrations` | Confirmed as a real routed page in frontend source; shared across integrations, extensions, and contextual data, which are all facets of the same catalog |
| `webhooks list`/`get`/`types`/`create`/`update`/`delete`/`test` | `{base}/#/extensions/outbound-webhooks` | Confirmed as a real routed page in frontend source |
| `iam api-keys list`/`get`/`send-data-keys`/`admin-list`/`create`/`update`/`delete`/`admin-delete`/`admin-set-status` | `{base}/#/settings/api-keys` | Confirmed as a real routed page in frontend source |
| `iam users search`/`get`/`create`/`update`/`set-status` | `{base}/#/settings/team/members` | Confirmed as a real routed page in frontend source; user create/update is a dialog on this flat list page with no per-user route |
| `iam ip-access get`/`create`/`update`/`delete` | `{base}/#/settings/login-access-policies` | Confirmed as a real routed page in frontend source |
| `ai-center applications list`/`get` (no create/update/delete in this CLI) | `{base}/#/ai-center/overview/application-catalog` | Confirmed as a real routed page in frontend source |
| `ai-center evaluations list`/`get`/`create`/`update`/`delete`, `ai-center custom-evaluations list`/`list-for-application`/`create`/`update`/`add`/`remove` | `{base}/#/ai-center/overview/eval-catalog` | Confirmed as a real routed page in frontend source |
| `olly ask` | `{base}/#/olly` | Confirmed as a real routed page in frontend source |

The console base URL (e.g. `https://acme.app.eu2.coralogix.com`) is resolved in this order:

1. **`console_url`** in the profile TOML, if set - used as-is (see the field table above). No API call is made in this case.
2. A known **console domain** for the profile's region (table below), combined with a team subdomain fetched from the profile's API. `cx` calls `GET /identity/whoami` (authenticated with the profile's own credentials - no extra permissions needed) and takes the subdomain from `team_url` only - `team_name` is a display name, not a URL label (e.g. a team named `acmeprod` could have the real subdomain `acme-prod`), so it is never used as a fallback to avoid producing a confidently wrong link. The value is lowercased and must consist only of ASCII letters, digits, and hyphens (valid hostname-label characters); if `team_url` is absent, or the resulting value doesn't qualify as a hostname label (e.g. it contains spaces or non-ASCII characters), no link is printed. This call is best-effort and never fails the underlying command - it also runs only if you didn't set `console_url`. Its result is cached per invocation, so printing multiple links in one command only calls `/identity/whoami` once.
3. **No link is printed** if the region has no known console domain (`Region::Custom`, and any other region without an entry in the table below), or if step 2's `/identity/whoami` call fails, or `team_url` is absent, or the team subdomain can't be extracted as described above.

| Region | Console domain |
|---|---|
| `us1` | `app.coralogix.us` |
| `us2` | `app.cx498.coralogix.com` |
| `us3` | `app.us3.coralogix.com` |
| `eu1` | `coralogix.com` |
| `eu2` | `app.eu2.coralogix.com` |
| `ap1` | `app.coralogix.in` |
| `ap2` | `app.coralogixsg.com` |
| `ap3` | `app.ap3.coralogix.com` |
| `stg1` | *(unknown - no link printed)* |

Set `console_url` explicitly in the profile TOML to override this table or to enable console links for regions with no known domain (e.g. `stg1`, or a `Custom` region running a self-hosted console).

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

> **Env-only mode:** when no profile file exists on disk but both `CX_API_KEY` (or `--api-key`) and `CX_REGION` (or `--region`) are supplied, `cx` runs without a profile file. This is convenient for ephemeral environments (CI runners, containers, ad-hoc scripts) where running `cx profiles add <name>` first would be a paper-cut.

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

The local HTTP callback listener used during `cx profiles add <name>` (OAuth path) binds one port from the following fixed allow-list, chosen at random:

```
21783  24861  27654  31847  38129
```

Ensure at least one of these ports is available when running `cx profiles add <name>`.
