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

## DataPrime documentation bundle

The `cx dataprime` subcommands read command and function help from `assets/dataprime_docs.yaml`, which is compiled into the binary at build time via `include_str!` in `src/commands/dataprime.rs`. Nothing is loaded from disk at runtime.

## Architecture

See [architecture.md](architecture.md) for the execution flow, command archetypes, error handling, and module map.
