# Development

## Build & Test

```bash
cargo build                         # Dev build
cargo build --release               # Release build (stripped, LTO)
cargo fmt                           # Format code
cargo clippy                        # Lint
cargo test                          # Run all tests
cargo test -- --ignored             # Run integration tests (requires ~/.cx)
cargo run -- <args>                 # Run CLI in dev mode
```

Rust toolchain is pinned to **1.94.1** via `rust-toolchain.toml`.

## DataPrime documentation bundle

The `cx dataprime` subcommands read command and function help from **`assets/dataprime_docs.yaml`**, which is **compiled into the binary** at build time (`include_str!` in `src/commands/dataprime.rs`). Nothing is loaded from disk at runtime.

## Architecture

See `CLAUDE.md` for a detailed architecture overview including execution flow, key modules, and output modes.
