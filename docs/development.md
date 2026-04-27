# Development

## Build and test

```bash
cargo build                         # Dev build
cargo build --release               # Release build (stripped, LTO)
cargo fmt                           # Format code
cargo clippy                        # Lint
cargo test                          # Run all tests
cargo test -- --ignored             # Run integration tests (system keyring required)
cargo run -- <args>                 # Run CLI in dev mode
```

Rust toolchain is pinned to **1.94.1** via `rust-toolchain.toml`.

Before submitting a PR, verify:

- `cargo fmt --check` is clean
- `cargo clippy` produces no warnings
- `cargo test` passes

## End-to-end tests

The `tests/e2e/` suite invokes the compiled `cx` binary against a real
Coralogix test team, sanity-checking every command and subcommand. All
e2e tests are `#[ignore]`d, so the default `cargo test` run skips them.

Set credentials one of two ways:

```bash
# Option A — environment
export CX_API_KEY=cxtp_...
export CX_REGION=stg1

# Option B — .env file in the repo root (gitignored)
cp .env.example .env
# edit values
```

Then run:

```bash
cargo test --test e2e -- --ignored --test-threads=1
```

Tests skip gracefully (with a `[e2e] skipping ...` log line) when
credentials are absent, or when the test team has no data for a
discovery step (e.g. no alerts to fetch). The suite is read-only
against the test team — mutating commands (`alerts create`,
`alerts enable`/`disable`) are intentionally not covered.

CI runs the suite on every push to `master` and via manual
`workflow_dispatch` (see `.github/workflows/e2e.yml`).

## DataPrime documentation bundle

The `cx dataprime` subcommands read command and function help from `assets/dataprime_docs.yaml`, which is compiled into the binary at build time via `include_str!` in `src/commands/dataprime.rs`. Nothing is loaded from disk at runtime.

## Architecture

See [architecture.md](architecture.md) for the execution flow, command archetypes, error handling, and module map.
