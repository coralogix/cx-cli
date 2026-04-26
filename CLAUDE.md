# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`cx` — a Rust CLI for querying Coralogix observability data (logs, metrics, traces, dashboards, alerts) from the terminal. Supports multi-profile fan-out, multiple output formats (text/json/agents), and AI-optimized result spilling.

## Build & Development

```bash
cargo build                         # Dev build
cargo build --release               # Release build (stripped, LTO)
cargo fmt                           # Format code
cargo clippy                        # Lint
cargo test                          # Run all tests
cargo test <test_name>              # Run a single test
cargo test -- --ignored             # Run integration tests (filesystem-dependent)
cargo test --test e2e -- --ignored --test-threads=1   # E2E vs. staging (needs CX_API_KEY)
cargo run -- <args>                 # Run CLI in dev mode
```

Rust toolchain is pinned to **1.94.1** via `rust-toolchain.toml`.

## Architecture

### Execution Flow

1. **CLI parsing** (`main.rs`) — Clap derive macros define the command tree
2. **Config resolution** (`config.rs`) — Loads `~/.cx/config.toml` + `~/.cx/profiles/*.toml`, resolves per-profile credentials and region endpoints
3. **Target building** (`execution.rs`) — Each profile becomes an `ExecutionTarget` wrapping a `ResolvedConfig` + `CxClient`
4. **Fan-out** (`execution.rs::fan_out`) — Runs the command handler concurrently across all targets
5. **Result merging** (`execution.rs::merge_tagged_results`) — Combines per-profile results, tags rows with profile names when multi-profile
6. **Output rendering** (`render.rs`) — Shared helpers for text tables, JSON, and TOON-encoded agents format
7. **Spilling** (`spill.rs`) — If output exceeds `max_dataprime_direct_output_size` (default 100KiB), writes to a temp file and returns the path

### Key Modules

- **`src/api/client.rs`** — `CxClient`: HTTP wrapper with Bearer auth, methods for REST (post/get) and NDJSON streaming
- **`src/api/dataprime.rs`** — DataPrime query API (logs & traces via NDJSON)
- **`src/api/metrics.rs`** — PromQL queries (instant, range, search, labels)
- **`src/api/semantic_search.rs`** — Semantic Search HTTP API (fields + metrics)
- **`src/commands/*.rs`** — Command implementations (logs, metrics, traces, dashboards, alerts, search-fields, profiles, cleanup, dataprime docs)
- **`src/time.rs`** — Parses relative timestamps (`now-1h`, `now - 3d`) and ISO-8601
- **`src/render.rs`** — Shared rendering helpers (`render_table`, `render_json`, `bool_display`, etc.) for text/JSON/agents output
- **`src/spill.rs`** — Large result spilling + `transform_for_agents()` (shrinks output for AI consumers)
- **`src/tier.rs`** — Storage tier enum (FrequentSearch vs Archive)
- **`src/error.rs`** — `CxError` enum (Auth, Api, Http, Json, Io)

### Output Modes

- **Text** — Human-readable tables via `tabled` + `colored`
- **JSON** — Pretty-printed raw API responses
- **Agents** — TOON-encoded, metadata-stripped, spill-aware format for AI consumption

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
| `/run-tests` | "run tests", "cargo test", "check CI" | Run tests, clippy, fmt — full verification |
| `/create-pr` | "create a PR", "open a pull request" | Create GitHub PR with auto-generated summary |

### Skill Coverage

Which CLI commands have user-facing skills in `skills/`:

| CLI Command | User-Facing Skill | Status |
|-------------|-------------------|--------|
| `cx logs` | `query-logs` | Covered |
| `cx spans` | `query-spans` | Covered |
| `cx metrics` | `metrics-query` | Covered |
| `cx alerts` | `cx-alerts` | Covered |
| `cx dataprime` | `dataprime` | Covered |
| `cx logs` (RUM) | `rum` | Covered |
| _(cross-signal)_ | `telemetry-querying` | Gateway skill |
| `cx dashboards` | — | Not covered |
| `cx search-fields` | — | Not covered |
| `cx profiles` | — | Not covered |
| `cx cleanup` | — | Not covered |

### Testing Expectations

All agent-authored code must pass before committing:

- **`cargo fmt`** — all code must be formatted
- **`cargo clippy`** — no warnings allowed
- **`cargo test`** — all existing unit and integration tests must pass

New commands must add tests at all three layers:

| Layer | Location | Purpose |
|-------|----------|---------|
| **Unit** | `src/api/<domain>.rs` `#[cfg(test)]` | API response deserialization (mandatory for REST commands) |
| **Integration** | `tests/<domain>.rs` (wiremock) | Command runner with mocked HTTP — covers fan-out, merge, render |
| **E2E** | `tests/e2e/<domain>.rs` (`#[ignore]`d) | Real `cx` binary against staging — sanity check exit + output |

E2E tests don't run by default; run them with
`cargo test --test e2e -- --ignored --test-threads=1` (requires
`CX_API_KEY`). See [docs/adding-a-command.md](docs/adding-a-command.md)
§ "Testing" for templates.

- **New skills** — verify skill triggers and reference file completeness

Use `/run-tests` to run the full check before committing.
