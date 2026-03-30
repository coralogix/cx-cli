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
cargo run -- <args>                 # Run CLI in dev mode
```

Rust toolchain is pinned to **1.94.1** via `rust-toolchain.toml`.

Proto files are generated at build time by `build.rs` using `tonic-build` with vendored `protoc`. After a fresh checkout, protos must be fetched first:
```bash
protofetch --output-proto-directory proto fetch
cargo build
```

## Architecture

### Execution Flow

1. **CLI parsing** (`main.rs`) — Clap derive macros define the command tree
2. **Config resolution** (`config.rs`) — Loads `~/.cx/config.toml` + `~/.cx/profiles/*.toml`, resolves per-profile credentials and region endpoints
3. **Target building** (`execution.rs`) — Each profile becomes an `ExecutionTarget` wrapping a `ResolvedConfig` + `CxClient`
4. **Fan-out** (`execution.rs::fan_out`) — Runs the command handler concurrently across all targets
5. **Result merging** (`execution.rs::merge_tagged_results`) — Combines per-profile results, tags rows with profile names when multi-profile
6. **Output rendering** — Text tables (`tabled`), raw JSON, or TOON-encoded agents format
7. **Spilling** (`spill.rs`) — If output exceeds `max_dataprime_direct_output_size` (default 100KiB), writes to a temp file and returns the path

### Key Modules

- **`src/api/client.rs`** — `CxClient`: HTTP wrapper with Bearer auth, methods for REST (post/get) and NDJSON streaming
- **`src/api/dataprime.rs`** — DataPrime query API (logs & traces via NDJSON)
- **`src/api/metrics.rs`** — PromQL queries (instant, range, search, labels)
- **`src/api/schema_store.rs`** — gRPC client for semantic field/metric lookup
- **`src/api/openai.rs`** — Embedding generation for semantic search
- **`src/commands/*.rs`** — Command implementations (logs, metrics, traces, dashboards, alerts, search-fields, configure, cleanup, dataprime docs)
- **`src/time.rs`** — Parses relative timestamps (`now-1h`, `now - 3d`) and ISO-8601
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

`docs/` contains detailed reference documentation: [configuration](docs/configuration.md), [agents output format](docs/agents-output.md), [multi-profile fan-out](docs/multi-profile.md), [time syntax](docs/time-syntax.md), and [development guide](docs/development.md).

### Scripts

`scripts/generate_dataprime_docs.py` — Parses official DataPrime docs JSON into YAML for offline browsing.
