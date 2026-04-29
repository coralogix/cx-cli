# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`cx` - a Rust CLI for querying Coralogix observability data (logs, metrics, traces, dashboards, alerts) from the terminal. Supports multi-profile fan-out, multiple output formats (text/json/agents), and AI-optimized result spilling.

## Build & Development

```bash
cargo build                         # Dev build
cargo build --release               # Release build (stripped, LTO)
cargo fmt                           # Format code
cargo clippy                        # Lint
cargo test                          # Run all tests
cargo test <test_name>              # Run a single test
cargo test -- --ignored             # Run integration tests (filesystem-dependent)
cargo test --test e2e -- --ignored --test-threads=1   # E2E vs. Coralogix test team (needs CX_API_KEY)
cargo run -- <args>                 # Run CLI in dev mode
```

Rust toolchain is pinned to **1.94.1** via `rust-toolchain.toml`.

## Command Hierarchy

The CLI is organized into 26 commands grouped by domain. `cx --help` shows this layout:

```
Query:
  logs               Query logs using DataPrime syntax
  spans              Query spans using DataPrime syntax
  metrics            Query metrics using PromQL
  dataprime          DataPrime language reference and raw queries
  search-fields      Search log/span fields semantically

Observe:
  dashboards         Manage dashboards and dashboard folders
  views              Manage saved views and view folders
  slos               Manage SLO definitions

Detect & Respond:
  alerts             Manage alert definitions and schedulers
  incidents          Manage and triage incidents

Notifications:
  notifications      Manage connectors, routers, presets, and notification testing
  webhooks           Manage outgoing webhooks and automation actions

Data Pipeline:
  rules              Manage log parsing rule groups
  enrichments        Manage enrichment rules and custom enrichment tables
  e2m                Manage Events2Metrics definitions
  recording-rules    Manage Prometheus recording rule groups

Cost & Storage:
  usage              View data usage and consumption metrics
  tco                Manage TCO policies and settings
  retentions         Manage data retention settings
  quotas             Manage quota rules
  archive            Manage data archive storage configuration

Integrations:
  integrations       Manage integrations, extensions, and contextual data

Access:
  iam                Manage API keys, roles, scopes, users, groups, SAML, and IP access

Agent:
  schema             Output the full command tree as JSON for agent consumption

Local:
  profiles           Manage profiles (list, add, delete, set-default)
  cleanup            Remove stale temp files
```

**Agent discovery:** `cx schema` outputs the full command tree (commands, subcommands, flags, descriptions) as JSON. Agents should call `cx schema` to discover available commands rather than parsing help text.

**Key renames from prior flat layout:**
- `rule-groups` -> `rules`
- `tco-policies` -> `tco`
- `quota-rules` -> `quotas`
- `data-usage` -> `usage`
- `data-archive` -> `archive`

**Wrapper commands** (merged from multiple former top-level commands):
- `notifications` = connectors + routers + presets + notification-test
- `webhooks` = outgoing webhooks + actions (formerly `actions`)
- `enrichments` = enrichment rules + custom enrichment tables (formerly `custom-enrichments`)
- `integrations` = extensions + contextual-data
- `iam` = api-keys + roles + scopes + users + team-groups + saml + ip-access

## Architecture

### Execution Flow

1. **CLI parsing** (`main.rs`) - Clap derive macros define the command tree
2. **Config resolution** (`config.rs`) - Loads `~/.cx/config.toml` + `~/.cx/profiles/*.toml`, resolves per-profile credentials and region endpoints
3. **Target building** (`execution.rs`) - Each profile becomes an `ExecutionTarget` wrapping a `ResolvedConfig` + `CxClient`
4. **Fan-out** (`execution.rs::fan_out`) - Runs the command handler concurrently across all targets
5. **Result merging** (`execution.rs::merge_tagged_results`) - Combines per-profile results, tags rows with profile names when multi-profile
6. **Output rendering** (`render.rs`) - Shared helpers for text tables, JSON, and TOON-encoded agents format
7. **Spilling** (`spill.rs`) - If output exceeds `max_dataprime_direct_output_size` (default 100KiB), writes to a temp file and returns the path

### Layout

Each CLI command owns a directory under `src/commands/<command>/`. The directory holds the command handler (`mod.rs`) and, for REST commands, an HTTP-client module (`api.rs`). The dataprime command also owns `semantic_search.rs`, which is shared with the platform-owned `search-fields` command. The shared HTTP base client lives at `src/api_client.rs`. Cross-cutting infrastructure stays at the top of `src/`.

This per-command layout drives `CODEOWNERS`: each domain in the file maps directly to the command directories it owns (see CODEOWNERS comments).

### Key Modules

- **`src/api_client.rs`** - `CxClient`: HTTP wrapper with Bearer auth, methods for REST (post/get) and NDJSON streaming
- **`src/commands/dataprime/api.rs`** - DataPrime query API (logs & traces via NDJSON)
- **`src/commands/dataprime/semantic_search.rs`** - Semantic Search HTTP API (fields + metrics)
- **`src/commands/metrics/api.rs`** - PromQL queries (instant, range, search, labels)
- **`src/commands/<command>/mod.rs`** - Per-command handler (logs, metrics, spans, dashboards, alerts, notifications, webhooks, enrichments, rules, tco, quotas, usage, archive, integrations, iam, slos, search-fields, profiles, cleanup, dataprime docs, schema)
- **`src/time.rs`** - Parses relative timestamps (`now-1h`, `now - 3d`) and ISO-8601
- **`src/render.rs`** - Shared rendering helpers (`render_table`, `render_json`, `bool_display`, etc.) for text/JSON/agents output
- **`src/spill.rs`** - Large result spilling + `transform_for_agents()` (shrinks output for AI consumers)
- **`src/tier.rs`** - Storage tier enum (FrequentSearch vs Archive)
- **`src/error.rs`** - `CxError` enum (Auth, Api, Http, Json, Io)

### Output Modes

- **Text** - Human-readable tables via `tabled` + `colored`
- **JSON** - Pretty-printed raw API responses
- **Agents** - TOON-encoded, metadata-stripped, spill-aware format for AI consumption

### Config & Environment

Config lives in `~/.cx/`. Environment variables `CX_PROFILE`, `CX_API_KEY`, `CX_REGION`, `OPENAI_API_KEY` override profile settings.

### Skills

`skills/` contains Claude Code skill plugins for AI-driven observability investigation (alerts, metrics queries, telemetry querying).

### Documentation

**Contributor guides:** [architecture](docs/architecture.md), [adding a command](docs/adding-a-command.md), [adding a skill](docs/adding-a-skill.md), [development](docs/development.md)

**Reference docs:** [configuration](docs/configuration.md), [agents output format](docs/agents-output.md), [multi-profile fan-out](docs/multi-profile.md), [time syntax](docs/time-syntax.md)

## Contributing

### Development Skills

The `.claude/skills/` directory contains workflow skills for agents developing cx itself:

| Skill | Trigger | Purpose |
|-------|---------|---------|
| `/add-command` | "add a command", "implement cx ..." | End-to-end workflow for adding a new CLI command |
| `/add-skill` | "add a skill", "create a skill" | End-to-end workflow for creating a user-facing skill |
| `/run-tests` | "run tests", "cargo test", "check CI" | Run tests, clippy, fmt - full verification |
| `/create-pr` | "create a PR", "open a pull request" | Create GitHub PR with auto-generated summary |

### Skill Coverage

Which CLI commands have user-facing skills in `skills/`:

| CLI Command | User-Facing Skill | Status |
|-------------|-------------------|--------|
| `cx logs` | `cx-query-logs` | Covered |
| `cx spans` | `cx-query-spans` | Covered |
| `cx metrics` | `cx-metrics-query` | Covered |
| `cx alerts` | `cx-alerts` | Covered |
| `cx dataprime` | `cx-dataprime` | Covered |
| `cx logs` (RUM) | `cx-rum` | Covered |
| _(cross-signal)_ | `cx-telemetry-querying` | Gateway skill |
| `cx dashboards` | `cx-create-dashboard` | Covered |
| `cx search-fields` | `cx-telemetry-querying` | Covered (via gateway) |
| `cx usage` | `cx-cost-optimization` | Covered |
| `cx tco` | `cx-cost-optimization` | Covered |
| `cx retentions` | `cx-cost-optimization` | Covered |
| `cx quotas` | `cx-cost-optimization` | Covered |
| `cx archive` | `cx-cost-optimization` | Covered |
| `cx incidents` | `cx-incident-management` | Covered |
| `cx slos` | `cx-incident-management` | Covered |
| `cx rules` | `cx-data-pipeline` | Covered |
| `cx enrichments` | `cx-data-pipeline` | Covered |
| `cx e2m` | `cx-data-pipeline` | Covered |
| `cx recording-rules` | `cx-data-pipeline` | Covered |
| `cx iam` | `cx-platform-admin` | Covered |
| `cx views` | `cx-observability-setup` | Covered |
| `cx webhooks` | `cx-observability-setup` | Covered |
| `cx notifications` | `cx-observability-setup` | Covered |
| `cx integrations` | `cx-observability-setup` | Covered |
| `cx schema` | `cx-telemetry-querying` | Covered (via gateway) |
| `cx profiles` | - | Local command |
| `cx cleanup` | - | Local command |

### Testing Expectations

All agent-authored code must pass before committing:

- **`cargo fmt`** - all code must be formatted
- **`cargo clippy`** - no warnings allowed
- **`cargo test`** - all existing unit and integration tests must pass

New commands must add tests at all three layers:

| Layer | Location | Purpose |
|-------|----------|---------|
| **Unit** | `src/commands/<command>/api.rs` `#[cfg(test)]` | API response deserialization (mandatory for REST commands) |
| **Integration** | `tests/<command>/main.rs` (wiremock) | Command runner with mocked HTTP - covers fan-out, merge, render |
| **E2E** | `tests/e2e/<command>/mod.rs` (`#[ignore]`d) | Real `cx` binary against the Coralogix test team - sanity check exit + output |

E2E tests don't run by default; run them with
`cargo test --test e2e -- --ignored --test-threads=1` (requires
`CX_API_KEY`). See [docs/adding-a-command.md](docs/adding-a-command.md)
§ "Testing" for templates.

- **New skills** - verify skill triggers and reference file completeness

Use `/run-tests` to run the full check before committing.
