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

## Proto Generation

Proto files are generated at build time by `build.rs` using `tonic-build` with vendored `protoc`. After a fresh checkout, protos must be fetched first:

```bash
protofetch --output-proto-directory proto fetch
cargo build
```

The fetched `.proto` files live under `proto/` which is gitignored.

## DataPrime Documentation Setup

The `cx dataprime` commands require a documentation file at `~/.cx/dataprime_docs.yaml`, generated from the official Coralogix DataPrime documentation JSON.

```bash
# Generate from a local dataprime_docs.json file
python3 scripts/generate_dataprime_docs.py --input /path/to/dataprime_docs.json

# Write to stdout (for CI pipelines)
python3 scripts/generate_dataprime_docs.py --input /path/to/dataprime_docs.json --stdout

# Write to a custom output path
python3 scripts/generate_dataprime_docs.py --input /path/to/dataprime_docs.json --output /custom/path.yaml
```

The `dataprime_docs.json` source file can be obtained from internal Coralogix sources. The script extracts ~30-40 DataPrime commands and ~100-150 functions.

All JSON parsing logic lives in the Python script. The Rust binary only reads the pre-parsed YAML file, allowing documentation updates without rebuilding the binary.

## Architecture

See `CLAUDE.md` for a detailed architecture overview including execution flow, key modules, and output modes.
