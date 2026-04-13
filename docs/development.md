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

The `cx dataprime` subcommands load YAML produced by **`generate_dataprime_docs.py`** from **[internal-dataprime-docs-generator](https://github.com/coralogix/internal-dataprime-docs-generator)**. That script downloads fresh `dataprime_docs.json` when run without `--input` (unless you pass `--input` for an offline file), writes YAML, and you commit **`assets/dataprime_docs.yaml`** here.

- **Embedded only** — `assets/dataprime_docs.yaml` is compiled into the binary (`include_str!`). To ship newer docs, regenerate that file and rebuild `cx`; there is no runtime override path.

### Regenerating `assets/dataprime_docs.yaml`

You can either **run `generate_dataprime_docs.py` yourself** (below) **or skip that** and **copy the YAML from [internal-dataprime-docs-generator](https://github.com/coralogix/internal-dataprime-docs-generator)**: whenever the upstream DataPrime documentation changes, automation regenerates the bundle under **`output/`** in that repository ([`output/`](https://github.com/coralogix/internal-dataprime-docs-generator/tree/master/output) on `master`). Use that file as the source for **`assets/dataprime_docs.yaml`** here instead of running the generator locally.

**Run the generator locally** — clone [internal-dataprime-docs-generator](https://github.com/coralogix/internal-dataprime-docs-generator) wherever you like and pass **`--output`** with the path to **this** repository’s `assets/dataprime_docs.yaml` (absolute or relative to your shell cwd).

**From a local clone** (replace the paths with yours):

```bash
python3 /path/to/internal-dataprime-docs-generator/generate_dataprime_docs.py \
  --output /path/to/coralogix-cli/assets/dataprime_docs.yaml
```

Then, in your `coralogix-cli` checkout:

```bash
cargo test   # unit tests parse the embedded YAML; fix any failures before committing
```

If you need a **fixed** JSON file (air‑gapped or debugging), pass `--input /path/to/dataprime_docs.json` to the generator instead of relying on the download.

## Architecture

See `CLAUDE.md` for a detailed architecture overview including execution flow, key modules, and output modes.
