# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`cx` - a Rust CLI for querying Coralogix observability data (logs, metrics, traces, dashboards, alerts) from the terminal. Supports multi-profile fan-out, multiple output formats (text/json/toon), and AI-optimized result spilling.

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

Rust toolchain is pinned to **1.96.1** via `rust-toolchain.toml`.

## Command Hierarchy

The CLI is organized into 29 commands grouped by domain. `cx --help` shows this layout:

```
Query:
  logs               Query logs using DataPrime syntax
  spans              Query spans using DataPrime syntax
  metrics            Query metrics using PromQL
  dataprime          DataPrime language reference and raw queries
  docs               Search and fetch official Coralogix product documentation
  search-fields      Search log/span fields by description or value content

Observe:
  dashboards         Manage dashboards and dashboard folders
  views              Manage saved views and view folders
  slos               Manage SLO definitions
  infra              Query infrastructure resources and their data

AI:
  ai-center (risky)  Manage AI Center applications, evaluations, policies, and pricing

Detect & Respond:
  alerts             Manage alert definitions and suppression rules
  cases              Manage and triage cases

Notifications:
  notifications      Manage connectors, routers, presets, and notification testing
  webhooks           Manage outgoing webhooks and automation actions

Data Pipeline:
  parsing-rules      Manage log parsing rules
  enrichments        Manage enrichment rules and custom enrichment tables
  e2m                Manage Events2Metrics definitions
  recording-rules    Manage Prometheus recording rule groups

Cost & Storage:
  usage              View data usage and consumption metrics
  tco                Manage TCO policies and settings
  retentions         Manage data retention settings
  archive (risky)    Manage data archive storage configuration

Integrations:
  integrations       Manage integrations, extensions, and contextual data

Access:
  iam (risky)        Manage API keys, roles, scopes, users, groups, and IP access

Agent:
  schema             Output the full command tree as JSON for agent consumption

Local:
  profiles           Manage profiles (list, add, delete, set-default)
  skills             Install the cx agent skills for coding agents
  cleanup            Remove stale temp files
```

**`cx search-fields -s value`:** Calls Olly Knowledge Base `POST /api/v1/search-by-value` on the region API base URL. The platform ingress uses gateway permission `legacy-archive-queries:Execute` (AAA id 40); `dataset_type` in the JSON body (`logs` / `spans` / `all`) is still enforced in-app by the service. See `olly-knowledge-base` `apps/values-reader-service/AGENTS.md` and `platform/defaults/networking/defaults.yaml`.

**`cx dashboards` semantic search:** Three Olly Knowledge Base-powered subcommands for discovering dashboards and queries:
- `cx dashboards search <description>` — Find dashboards matching a natural-language description
- `cx dashboards query-search --description <query-text>` — Search dashboard query content semantically
- `cx dashboards query-search --field <field-path>` — Find all queries referencing a specific field (e.g., `$d.http.status_code`)

All use the Olly KB semantic-search-service API with gateway permission `legacy-archive-queries:Execute` (AAA id 40). See `olly-knowledge-base` `apps/semantic-search-service/AGENTS.md`.

**`cx skills install`:** Installs the cx agent skills bundle by shelling out to the vercel-labs `skills` npx installer (`npx skills add coralogix/cx-cli/skills` — the `skills/` subdir, so the contributor-only dev skills under `.claude/skills/` are never installed for end users). By default it asks one question (global vs local scope, skipped by `--global`/`--local`), then runs fully non-interactively with agent auto-detection; `--agent <name>` overrides auto-detect, `--interactive` walks the installer's full flow. Requires Node.js/npx; the CLI is fully usable without skills. Logic lives in `src/commands/skills/mod.rs`.

**Agent discovery:** `cx schema` outputs the full command tree (commands, subcommands, flags, descriptions) as JSON. Agents should call `cx schema` to discover available commands rather than parsing help text.

**Key renames from prior flat layout:**
- `rule-groups` -> `parsing-rules`
- `tco-policies` -> `tco`
- `data-usage` -> `usage`
- `data-archive` -> `archive`

**Wrapper commands** (merged from multiple former top-level commands):
- `notifications` = connectors + routers + presets + notification-test
- `webhooks` = outgoing webhooks + actions (formerly `actions`)
- `enrichments` = enrichment rules + custom enrichment tables (formerly `custom-enrichments`)
- `integrations` = extensions + contextual-data
- `iam` = api-keys + roles + scopes + users + team-groups + ip-access

**Risky commands:** `iam`, `archive`, and `ai-center` are marked `(risky)` in help output. All write operations (create, update, delete, enable, disable, set, set-status) under these commands require interactive confirmation. Pass `--yes` to skip the prompt (e.g., in scripts or CI). Non-interactive terminals without `--yes` get a clear error. The confirmation logic lives in `src/safety.rs`.

## Architecture

### Execution Flow

1. **CLI parsing** (`main.rs`) - Clap derive macros define the command tree
2. **Config resolution** (`config.rs`) - Loads `~/.cx/config.toml` + `~/.cx/profiles/*.toml`, resolves per-profile credentials and region endpoints
3. **Target building** (`execution.rs`) - Each profile becomes an `ExecutionTarget` wrapping a `ResolvedConfig` + `CxClient`
4. **Fan-out** (`execution.rs::fan_out`) - Runs the command handler concurrently across all targets
5. **Result merging** (`execution.rs::merge_tagged_results`) - Combines per-profile results, tags rows with profile names when multi-profile
6. **Output rendering** (`render.rs`) - Shared helpers for text tables, JSON, and TOON-encoded toon output
7. **Spilling** (`spill.rs`) - If output exceeds `max_dataprime_direct_output_size` (default 100KiB), writes to a temp file and returns the path

### Layout

Each CLI command owns a directory under `src/commands/<command>/`. The directory holds the command handler (`mod.rs`) and, for REST commands, an HTTP-client module (`api.rs`). The dataprime command also owns `semantic_search.rs`, which is shared with the platform-owned `search-fields` command. The shared HTTP base client lives at `src/api_client.rs`. Cross-cutting infrastructure stays at the top of `src/`.

This per-command layout drives `CODEOWNERS`: each domain in the file maps directly to the command directories it owns (see CODEOWNERS comments).

### Key Modules

- **`src/safety.rs`** - `confirm_destructive()`: interactive confirmation for risky write operations; `is_write_verb()`: centralized write command detection; `get_leaf_subcommand_name()` / `get_top_level_subcommand_name()`: ArgMatches tree walking
- **`src/api_client.rs`** - `CxClient`: HTTP wrapper with Bearer auth, methods for REST (post/get) and NDJSON streaming
- **`src/commands/dataprime/api.rs`** - DataPrime query API (logs & traces via NDJSON)
- **`src/commands/dataprime/semantic_search.rs`** - Semantic Search HTTP API (fields + metrics)
- **`src/commands/metrics/api.rs`** - PromQL queries (instant, range, search, labels)
- **`src/commands/<command>/mod.rs`** - Per-command handler (logs, metrics, spans, dashboards, alerts, notifications, webhooks, enrichments, parsing-rules, tco, usage, archive, integrations, iam, slos, search-fields, profiles, cleanup, dataprime docs, schema)
- **`src/time.rs`** - Parses relative timestamps (`now-1h`, `now - 3d`) and ISO-8601
- **`src/render.rs`** - Shared rendering helpers (`render_table`, `render_json`, `bool_display`, etc.) for text/JSON/toon output
- **`src/spill.rs`** - Large result spilling + `transform_for_toon()` (shrinks output for AI consumers)
- **`src/tier.rs`** - Storage tier enum (FrequentSearch vs Archive)
- **`src/error.rs`** - `CxError` enum (Auth, Api, Http, Json, Io)

### Output Modes

- **Text** - Human-readable tables via `tabled` + `colored`
- **JSON** - Pretty-printed raw API responses
- **Agents** - TOON-encoded, metadata-stripped, spill-aware format for AI consumption

### Config & Environment

Config lives in `~/.cx/`. Environment variables `CX_PROFILE`, `CX_API_KEY`, `CX_REGION`, `OPENAI_API_KEY` override profile settings.

### Skills

`skills/` contains Claude Code skill plugins for AI-driven observability investigation. Ten skills cover all CLI commands: `cx-telemetry-querying` (logs, spans, metrics, RUM, DataPrime — gateway with pillar-specific reference files), `cx-alerts`, `cx-cases`, `cx-slos`, `cx-dashboards`, `cx-infra`, `cx-cost-optimization`, `cx-data-pipeline`, `cx-observability-setup`, and `cx-platform-admin`. Shared reference files (DataPrime syntax, PromQL guidelines, telemetry-pillar how-tos) live in `skills/shared/` and are distributed to consuming skills via `scripts/sync-shared-references.sh`.

### Documentation

**Contributor guides:** [architecture](contributing/architecture.md), [adding a command](contributing/adding-a-command.md), [adding a skill](contributing/adding-a-skill.md), [development](contributing/development.md)

**Reference docs:** [configuration](docs/configuration.md), [TOON output format](docs/toon-output.md), [multi-profile fan-out](docs/multi-profile.md), [time syntax](docs/time-syntax.md)

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
| `cx logs` | `cx-telemetry-querying` | Covered (loads `logs-querying.md` + `dataprime-reference.md`) |
| `cx spans` | `cx-telemetry-querying` | Covered (loads `spans-querying.md` + `dataprime-reference.md`) |
| `cx metrics` | `cx-telemetry-querying` | Covered (loads `metrics-querying.md` + `promql-guidelines.md`) |
| `cx dataprime` | `cx-telemetry-querying` | Covered (loads `dataprime-reference.md`) |
| `cx docs` | `coralogix-docs` | Covered |
| `cx logs` (RUM) | `cx-telemetry-querying` | Covered (loads `rum-querying.md` + `rum-fields.md` + `dataprime-reference.md`) |
| `cx search-fields` | `cx-telemetry-querying` | Covered (via gateway) |
| `cx schema` | `cx-telemetry-querying` | Covered (via gateway) |
| `cx alerts` | `cx-alerts` | Covered |
| `cx dashboards` | `cx-dashboards` | Covered |
| `cx usage` | `cx-cost-optimization` | Covered |
| `cx tco` | `cx-cost-optimization` | Covered |
| `cx retentions` | `cx-cost-optimization` | Covered |
| `cx archive` | `cx-cost-optimization` | Covered |
| `cx cases` | `cx-cases` | Covered |
| `cx ai-center` | `cx-ai-center` | Covered (loads `ai-center-queries.md` + `dataprime-reference.md` + `spans-querying.md`) |
| `cx slos` | `cx-slos` | Covered |
| `cx parsing-rules` | `cx-data-pipeline` | Covered |
| `cx enrichments` | `cx-data-pipeline` | Covered |
| `cx e2m` | `cx-data-pipeline` | Covered |
| `cx recording-rules` | `cx-data-pipeline` | Covered |
| `cx iam` | `cx-platform-admin` | Covered |
| `cx views` | `cx-observability-setup` | Covered |
| `cx webhooks` | `cx-observability-setup` | Covered |
| `cx notifications` | `cx-observability-setup` | Covered |
| `cx integrations` | `cx-observability-setup` | Covered |
| `cx schema` | `cx-telemetry-querying` | Covered (via gateway) |
| `cx olly` | `cx-olly` | Covered |
| `cx infra` | `cx-infra` | Covered |
| `cx profiles` | - | Local command |
| `cx skills` | - | Local command (installs the skills in this table) |
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
`CX_API_KEY`). See [contributing/adding-a-command.md](contributing/adding-a-command.md)
§ "Testing" for templates.

- **New skills** - verify skill triggers and reference file completeness

Use `/run-tests` to run the full check before committing.
