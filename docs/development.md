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

The `cx dataprime` subcommands load YAML produced by **`generate_dataprime_docs.py`** from the sibling repository **[dataprime-docs-generator](https://github.com/coralogix/dataprime-docs-generator)** (clone as `../dataprime-docs-generator` next to this repo). That script downloads fresh `dataprime_docs.json` when run without `--input` (unless you pass `--input` for an offline file), writes YAML, and you commit **`assets/dataprime_docs.yaml`** here.

- **Embedded only** — `assets/dataprime_docs.yaml` is compiled into the binary (`include_str!`). To ship newer docs, regenerate that file and rebuild `cx`; there is no runtime override path.

### Regenerating `assets/dataprime_docs.yaml` (local sibling layout)

With `coralogix-cli` and `dataprime-docs-generator` as sibling directories under the same parent:

```bash
cd ../dataprime-docs-generator
# Writes ~/.cx/dataprime_docs.yaml (default) and copies the same YAML into this repo
python3 generate_dataprime_docs.py --also-output ../coralogix-cli/assets/dataprime_docs.yaml

cd ../coralogix-cli
cargo test   # unit tests parse the embedded YAML; fix any failures before committing
```

### Regenerating from a published clone

If you only have a Git clone of `dataprime-docs-generator` elsewhere, use its path in `--output` (absolute or relative), or `curl` the raw `generate_dataprime_docs.py` from GitHub and run it with `--output /path/to/coralogix-cli/assets/dataprime_docs.yaml`.

If you need a **fixed** JSON file (air‑gapped or debugging), pass `--input /path/to/dataprime_docs.json` to the generator instead of relying on the download.

## Architecture

See `CLAUDE.md` for a detailed architecture overview including execution flow, key modules, and output modes.
