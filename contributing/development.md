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

Rust toolchain is pinned to **1.96.1** via `rust-toolchain.toml`.

Before submitting a PR, verify:

- `cargo fmt --check` is clean
- `cargo clippy` produces no warnings
- `cargo test` passes

## End-to-end tests

The `tests/e2e/` suite invokes the compiled `cx` binary against a real
Coralogix team and covers every command and subcommand, including
create/update/delete. All e2e tests are `#[ignore]`d, so the default
`cargo test` run skips them.

### The writable test team

E2E runs against a dedicated team (`cxai-otel-demo`, region `eu2`) with
endless retention and a continuous feed of `otel-demo` logs, spans and
metrics. The suite may create, update and delete resources there. It must
never be pointed at a customer or shared team.

Set credentials one of two ways:

```bash
# Option A - environment
export CX_API_KEY=cxtp_...
export CX_REGION=eu2
export CX_E2E_ALLOW_WRITES=1     # write tests are skipped without it

# Option B - .env file in the repo root (gitignored)
cp .env.example .env
# edit values
```

Then run:

```bash
cargo test --test e2e -- --ignored                 # whole suite, parallel
cargo test --test e2e views_ -- --ignored          # one domain
cargo test --test e2e views_lifecycle -- --ignored --nocapture   # see cx output
```

Tests skip gracefully (with a `[e2e] skipping ...` log line) when
credentials are absent, when `CX_E2E_ALLOW_WRITES` is unset (write tests
only), or when the team has no data for a discovery step.

### Parallel safety

PRs run the suite concurrently against the same team, so tests never
depend on shared state:

- Every created resource is named with `harness::unique_name("kind")`,
  which yields `cx-e2e-<kind>-<YYYYmmddHHMM>-<run id>-<seq>`. The run id
  comes from `CX_E2E_RUN_ID` (CI sets it from the workflow run) or is
  random.
- A test asserts only on resources it created (it finds them by that
  unique name) and deletes them before it ends. `harness::Cleanup` runs
  the delete even when an assertion panics.
- Global, singleton settings (retention, archive target, TCO settings,
  model pricing, ...) are only round-tripped: read, write the same value
  back, read again. Anything that cannot be round-tripped safely is
  listed as `[e2e] not covered: ...` in the test file with the reason.
- `tests/e2e/sweeper.rs` lists every resource type and deletes
  `cx-e2e-*` names from other runs older than two hours. CI runs it before
  the suite, so a crashed runner never leaves garbage behind for long.

### CI

`.github/workflows/e2e.yml` runs the sweeper and then the suite on every
pull request (code paths only) and every push to `master`. Different PRs
run in parallel; a new push to the same PR cancels its older run. The
workflow reads the repository secret `CX_E2E_WRITE_API_KEY` (an API key of
the writable test team with full permissions); the region is set in the
workflow file. The older `CX_E2E_API_KEY` secret (read-only team) is no
longer used.

## DataPrime documentation bundle

The `cx dataprime` subcommands read command and function help from `assets/dataprime_docs.yaml`, which is compiled into the binary at build time via `include_str!` in `src/commands/dataprime/mod.rs`. Nothing is loaded from disk at runtime.

## Architecture

See [architecture.md](architecture.md) for the execution flow, command archetypes, error handling, and module map.
