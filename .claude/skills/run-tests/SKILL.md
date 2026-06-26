---
name: run-tests
description: Use this skill when the user asks to "run tests", "test this", "check if tests pass", "cargo test", "run clippy", "lint this", "check formatting", "cargo fmt", "CI checks", "verify changes", "does this pass tests", "run the full check", "pre-commit check", or wants to verify that code changes are correct. Use this even when the user says something like "make sure this works" or "check for issues" in the context of code changes.
version: 0.1.0
metadata:
  internal: true
---

# Run Tests & Verify

How to run tests, lint, and verify changes in the `cx` CLI. This skill covers running only - for writing new tests, see the `add-command` skill.

## Quick Reference

| Command | What it does |
|---------|-------------|
| `cargo test` | Run all unit + integration tests |
| `cargo test <name>` | Run tests matching a name substring |
| `cargo test -- --ignored` | Run integration tests (filesystem-dependent, skipped by default) |
| `cargo fmt --check` | Check formatting (no changes) |
| `cargo fmt` | Auto-fix formatting |
| `cargo clippy --locked -- -D warnings` | Lint with warnings-as-errors |
| `cargo build` | Verify compilation |

## Full CI Check

This is the exact sequence CI runs on every PR. Run it before committing or creating a PR:

```bash
cargo fmt --check && cargo clippy --locked -- -D warnings && cargo test --locked
```

The `--locked` flag ensures the lockfile is respected - this matches `.github/workflows/test.yml` and `lint.yml` exactly. CI runs tests on Ubuntu, macOS, and Windows.

## What to Run After a Change

Pick the right level of verification based on what changed:

- **Any `.rs` file** - `cargo test` + `cargo clippy --locked -- -D warnings`
- **Formatting only** - `cargo fmt --check` (or `cargo fmt` to auto-fix)
- **`Cargo.toml`** - full CI check (dependency changes affect everything)
- **API types (`src/commands/<cmd>/api.rs`)** - run that command's tests first (`cargo test alerts`, `cargo test dataprime`), then the full suite
- **Test files only** - `cargo test <relevant_test_name>`

## Running Specific Tests

Target tests efficiently instead of running the entire suite:

- `cargo test alerts` - all tests with "alerts" in the name
- `cargo test --test dataprime` - only the `tests/dataprime/` integration test binary (input + output + query)
- `cargo test --test dataprime input` - only the `input` submodule of that binary
- `cargo test config::tests` - only the tests in the `config` module
- `cargo test metrics::tests` - only metrics formatting tests

## Interpreting Failures

**Clippy warnings** - treated as errors via `-D warnings`. Fix every warning before committing. Common ones: unused variables, unnecessary clones, missing error handling.

**Format failures** - run `cargo fmt` to auto-fix. These never need manual intervention.

**Deserialization test failures** - usually means struct field names don't match JSON keys. Check that `#[serde(rename_all = "camelCase")]` is on response types and that `#[serde(default)]` is on `Vec` fields (the API sometimes omits empty arrays).

**Compilation errors in tests** - likely caused by changing a public function signature. Check callers in both `src/` and `tests/` directories.

**Integration test failures (`--ignored`)** - these depend on filesystem state (`~/.cx/` config directory). They may fail if the local environment isn't set up. This is expected in CI where no config exists.
