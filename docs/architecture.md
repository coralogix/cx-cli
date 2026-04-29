# Architecture

> Contributor-facing architecture reference for `cx`. Start here before adding a command, debugging the execution pipeline, or reviewing a PR.

## Execution flow

Every `cx` invocation follows a seven-step pipeline:

```
CLI parsing ──> Config resolution ──> Target building ──> Fan-out
                                                            │
                                              ┌─────────────┘
                                              v
                                        Result merging ──> Output rendering ──> Spilling
```

| Step | Owner | Key function / type |
|------|-------|---------------------|
| 1. CLI parsing | `src/main.rs` | `Cli` / `Commands` (Clap derive) |
| 2. Config resolution | `src/config.rs` | `resolve_all()` → `Vec<ResolvedConfig>` |
| 3. Target building | `src/execution.rs` | `build_targets()` → `Vec<Arc<ExecutionTarget>>` |
| 4. Fan-out | `src/execution.rs` | `fan_out(targets, \|t\| async { ... })` |
| 5. Result merging | `src/execution.rs` or `src/commands/dataprime/mod.rs` | `merge_tagged_results()` / `merge_results()` |
| 6. Output rendering | Command module | `match output { Text \| Json \| Agents }` |
| 7. Spilling | `src/spill.rs` | `maybe_spill()` (agents mode, DataPrime only) |

### Step details

**1. CLI parsing** -- `main.rs` uses two separate Clap parsers. `ProfilesCli` handles `cx profiles` without global API flags (no credentials needed). `Cli` handles everything else with global `--profile`, `--api-key`, `--region`, and `--output` flags. Commands that don't need credentials (`cleanup`, `dataprime list/show`, `schema`) are dispatched early before config resolution.

The `Cli` struct uses a custom `help_template` with an `after_help` string to display grouped command categories (Query, Observe, Detect & Respond, etc.) instead of Clap's default flat subcommand listing. This provides a curated help experience without relying on `next_help_heading` or `flatten`.

**2. Config resolution** -- `resolve_all()` resolves one or more profile names into `ResolvedConfig` values. Each resolution loads `~/.cx/profiles/<name>.toml`, applies CLI/env overrides, and obtains a bearer token (API key from file/keyring, or OAuth with automatic refresh). Precedence: CLI flags > env vars > profile file > global config defaults.

**3. Target building** -- `build_targets()` wraps each `ResolvedConfig` in an `ExecutionTarget` that owns a pre-built `CxClient` HTTP client. Targets are `Arc`-wrapped for cheap cloning into async tasks.

**4. Fan-out** -- `fan_out()` runs the same async closure concurrently across all targets using `futures::join_all()`. Returns `Vec<(profile_name, Result<T>)>`. Errors are per-profile -- one failing profile doesn't block others.

**5-7.** Merging, rendering, and spilling differ by command archetype. See below.

## Command archetypes

Commands fall into two patterns. New commands should follow one of these.

### Archetype A: DataPrime-based

**Commands:** `logs`, `spans`, `dataprime query`

These delegate to the shared pipeline in `commands::dataprime::run_query()`, which handles fan-out, merge, and render generically. The command module provides only a source-specific text renderer.

```
commands::logs::run()
    └─> dataprime::run_query(targets, query, "logs", ..., Some(render_log_text))
            ├─> fan_out(targets, |t| execute_query(t, ...))
            ├─> merge_results(per_profile, include_profile)
            └─> render_results(merged, output, max_direct, temp_dir, text_renderer)
```

**Adding a new DataPrime command** requires:
1. A command directory (e.g., `src/commands/logs/`) with a `mod.rs` containing a text renderer function matching `fn(&MergedResults) -> Result<()>`
2. A `run()` function that calls `dataprime::run_query()` with the source name and renderer
3. CLI definition in `main.rs` and dispatch in the `match cli.command` block

**Reference:** `src/commands/logs/mod.rs` -- the entire module is ~130 lines, most of which is the text renderer. The `run()` function is a single delegation call.

### Archetype B: REST-based

**Commands:** `alerts`, `dashboards`, `metrics`, `search-fields`, `notifications`, `webhooks`, `rules`, `enrichments`, `integrations`, `iam`, `usage`, `tco`, `retentions`, `quotas`, `archive`, `slos`, `views`, `incidents`, `e2m`, `recording-rules`

These manage their own fan-out, merge, and render inline. Each subcommand function follows the same shape:

```
pub async fn run_list(targets, ..., output) -> Result<()> {
    let include_profile = targets.len() > 1;

    // Fan-out: call the API across all profiles
    let per_profile = fan_out(targets, |t| async move {
        let api = AlertsApi::new(&t.client);
        Ok(api.list().await?)
    }).await;

    // Merge: collect results, print errors to stderr
    let mut all_items = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => { /* filter, push to all_items */ }
            Err(e) => eprintln!("error from profile '{profile}': {e:#}"),
        }
    }

    // Render: match on output format
    match output {
        OutputFormat::Json => { /* serde_json::to_string_pretty */ }
        OutputFormat::Agents => { /* toon_encode */ }
        OutputFormat::Text => { /* Table::new(rows) */ }
    }
}
```

**Adding a new REST command** requires:
1. A command directory (`src/commands/<resource>/`) with:
   - `api.rs` -- a `<Resource>Api` struct wrapping `&CxClient`
   - `mod.rs` -- `run_<subcommand>()` functions, with `pub mod api;` at the top
2. CLI definition in `main.rs` and dispatch

**Reference:** `src/commands/dashboards/` -- clean example using `render::*` helpers for all three output formats.

### Wrapper enum pattern

Several CLI commands group multiple related domains under a single top-level command. These use a **wrapper enum** in `main.rs` that nests further subcommand enums:

```rust
// Top-level wrapper in Commands enum
Iam {
    #[command(subcommand)]
    cmd: IamCmd,
},

// Wrapper enum groups related sub-domains
#[derive(Subcommand)]
enum IamCmd {
    ApiKeys { #[command(subcommand)] cmd: ApiKeysCmd },
    Roles { #[command(subcommand)] cmd: RolesCmd },
    Scopes { #[command(subcommand)] cmd: ScopesCmd },
    Users { #[command(subcommand)] cmd: UsersCmd },
    Groups { #[command(subcommand)] cmd: GroupsCmd },
    Saml { #[command(subcommand)] cmd: SamlCmd },
    IpAccess { #[command(subcommand)] cmd: IpAccessCmd },
}
```

This pattern is used by: `notifications` (connectors, routers, presets, test), `webhooks` (list/get/types + actions), `enrichments` (rules + custom), `integrations` (list/get + extensions + contextual-data), and `iam` (api-keys, roles, scopes, users, groups, saml, ip-access).

Each sub-domain still has its own API module (`src/api/<sub_domain>.rs`) and command module (`src/commands/<sub_domain>.rs`). The wrapper only affects CLI wiring and dispatch in `main.rs`.

### Help display

The `Cli` struct uses a custom `help_template` with an `after_help` string to display grouped command categories rather than Clap's default flat subcommand listing. The `after_help` text is a manually maintained string that mirrors the `Commands` enum. When adding a new command, update the `after_help` string to place it in the correct category group.

## Destructive operation confirmation

Commands marked `(risky)` in `cx --help` (`iam`, `archive`) require interactive confirmation for write operations (create, update, delete, enable, disable, set). The confirmation logic lives in `src/confirm.rs`:

1. If `--yes` is passed, the operation proceeds immediately
2. If stdin is not a terminal, the operation fails with a message directing the user to pass `--yes`
3. Otherwise, `inquire::Confirm` prompts the user (default: No)

The `--yes` flag is global on the `Cli` struct and available as `yes` in all match arms. Each destructive subcommand is tagged `[requires --yes]` in its doc comment, which surfaces in `--help` output.

When adding new destructive operations, call `confirm_destructive(message, yes)` before the handler invocation. See [adding-a-command.md](adding-a-command.md) for the full pattern.

## Output rendering

All commands support three output formats controlled by `--output` / `OutputFormat`:

### Text

Human-readable output using `tabled` for tabular data and `colored` for severity/status highlighting. Multi-profile queries add a "Profile" column. REST commands use `render::render_table()` which dynamically builds tables via `tabled::builder::Builder` -- the Profile column is conditionally included based on `include_profile`, so no duplicate struct definitions are needed.

DataPrime commands use custom text renderers (e.g., `render_log_text`) that print `<timestamp> [<severity>] <message>` per row.

### JSON

Raw API responses pretty-printed via `serde_json::to_string_pretty`. No transformation applied.

### Agents

Token-optimized format for AI consumers:

1. **TOON encoding** -- all agents output uses `toon_format::encode_default()` for compact serialization
2. **Metadata stripping** (DataPrime only) -- `transform_for_agents()` renames keys (`metadata` -> `$m`, `labels` -> `$l`, `userData` -> `$d`) and removes noisy metadata fields (`branchid`, `priorityclass`, `*TimestampMicros`, etc.)
3. **Spilling** (DataPrime non-aggregates only) -- `maybe_spill()` checks serialized size against `max_dataprime_direct_output_size` (default 100 KiB). If exceeded, writes to `cx_results_<hash>.json` in `temp_dir` and prints the path instead.

## Multi-profile pattern

A single boolean controls profile-aware behavior throughout the codebase:

```rust
let include_profile = targets.len() > 1;
```

When `true`:
- **Merging:** `tag_rows()` / `merge_tagged_results()` inject `"profile": "<name>"` into each JSON row
- **Text output:** commands use the multi-profile row struct (with Profile column)
- **Errors:** printed to stderr with `"error from profile '<name>'"` prefix

When `false`:
- No profile field is injected
- Single-profile row struct is used
- Errors still print to stderr

The generic version lives in `execution.rs` (`tag_rows`, `merge_tagged_results`). DataPrime commands use a specialized `dataprime::merge_results()` that also handles `is_aggregate` detection and warning collection.

## Error handling

Two error systems coexist, each serving a different layer:

### API layer: `CxError`

Defined in `src/error.rs`. A typed enum used throughout `src/api/`:

| Variant | When | Example |
|---------|------|---------|
| `Auth(String)` | 401/403 responses | Invalid API key |
| `Api { status, message }` | Other non-2xx responses | 404 alert not found |
| `Http(reqwest::Error)` | Network failures | Connection refused |
| `Json(serde_json::Error)` | Deserialization failures | Unexpected response shape |
| `Io(std::io::Error)` | File I/O failures | Config file unreadable |

`CxError` enables pattern matching on specific API errors:

```rust
match api.get(&id).await {
    Ok(val) => Ok(val),
    Err(CxError::Api { status: 404, .. }) => Ok(api.get_by_version_id(&id).await?),
    Err(e) => Err(anyhow::Error::from(e)),
}
```

### Command layer: `anyhow::Result`

All command functions return `anyhow::Result`. `CxError` converts automatically via the `?` operator. Use `.context()` to add human-readable messages:

```rust
serde_yaml::from_str(yaml).context("Failed to parse embedded dataprime documentation")?;
```

### Fan-out errors

Errors during fan-out are **per-profile** and **non-fatal**:
- Printed to stderr: `eprintln!("error from profile '{profile}': {e:#}")`
- Successful profiles continue normally
- The command exits 0 if at least one profile succeeds

## The API client

`CxClient` (`src/api_client.rs`) is a thin `reqwest` wrapper pre-configured with Bearer auth:

| Method | Use case |
|--------|----------|
| `post_raw(path, body)` | NDJSON/streaming endpoints (DataPrime) -- returns raw text |
| `post<T>(path, body)` | Typed JSON endpoints -- deserializes response |
| `get<T>(path, params)` | GET with query params |
| `post_empty<T>(path, params)` | State-change endpoints (enable/disable) -- handles empty 200/204 |

All methods run through `checked_text()` which maps HTTP status codes to `CxError` variants.

API modules (`src/commands/<resource>/api.rs`) wrap `CxClient` in domain-specific structs:

```rust
pub struct AlertsApi<'a> {
    client: &'a CxClient,
}

impl<'a> AlertsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self { Self { client } }
    pub async fn list(&self) -> crate::error::Result<ListResponse> { ... }
}
```

## Naming conventions

| Element | Convention | Example |
|---------|-----------|---------|
| Source files | `snake_case.rs` | `search_fields.rs` |
| Command functions | `run_<subcommand>()` | `run_list()`, `run_get()`, `run_query()` |
| API structs | `<Resource>Api` | `AlertsApi`, `MetricsApi` |
| Render helpers | `render::render_table`, `render::render_json`, etc. | Shared text/JSON output |
| Command directory | `src/commands/<resource>/` | `mod.rs` for the handler, `api.rs` for REST commands |
| Error types | `CxError` for API, `anyhow::Result` for commands | -- |

## Module map

Each CLI command owns a directory under `src/commands/`. REST commands keep their HTTP client alongside the handler in `api.rs`. Cross-cutting infrastructure stays at the top of `src/`. The shared HTTP base lives at `src/api_client.rs`.

```
src/
├── main.rs              # CLI definition (Clap) + dispatch + help_template/after_help
├── lib.rs               # Module re-exports
├── api_client.rs        # CxClient HTTP wrapper (Bearer auth, REST + NDJSON)
├── confirm.rs           # Destructive operation confirmation (--yes flag handling)
├── config.rs            # Config/profile loading, resolution, Region enum
├── execution.rs         # ExecutionTarget, fan_out(), tag_rows(), merge_tagged_results()
├── render.rs            # Shared rendering helpers (render_table, render_json, bool_display, etc.)
├── error.rs             # CxError enum
├── spill.rs             # Agents output spilling + transform_for_agents()
├── time.rs              # Relative/absolute timestamp parsing
├── tier.rs              # Tier enum (FrequentSearch | Archive)
├── oauth.rs             # OAuth 2.0 + OIDC browser login flow
├── keyring_store.rs     # OS keyring read/write
├── api_client.rs        # CxClient HTTP wrapper
└── commands/
    ├── dataprime/       # Shared DataPrime pipeline + docs subcommands
    │   ├── mod.rs       #   handler + docs (list/show)
    │   ├── api.rs       #   DataPrime query API (NDJSON streaming)
    │   └── semantic_search.rs  # Semantic search API (used by metrics + search-fields)
    ├── logs/mod.rs      # Log query (DataPrime archetype)
    ├── spans/mod.rs     # Span query (DataPrime archetype)
    ├── metrics/         # PromQL commands (REST archetype)
    │   ├── mod.rs
    │   └── api.rs       #   PromQL query APIs
    ├── alerts/          # Alert management (REST archetype)
    │   ├── mod.rs
    │   └── api.rs       #   Alerts CRUD API
    ├── dashboards/      # Dashboard commands (REST archetype)
    │   ├── mod.rs
    │   └── api.rs       #   Dashboards API
    ├── search_fields/mod.rs  # Semantic field lookup (uses dataprime::semantic_search)
    ├── schema.rs        # cx schema - JSON command tree for agent discovery
    ├── profiles/mod.rs  # Profile management (no API calls)
    ├── cleanup/mod.rs   # Temp file cleanup (no API calls)
    └── ...              # One module per REST domain (incidents, notifications,
                         #   webhooks, rules, enrichments, integrations, iam,
                         #   usage, tco, retentions, quotas, archive, slos,
                         #   views, e2m, recording_rules)
```
