---
name: add-command
description: Use this skill when the user asks to "add a command", "implement cx <something>", "new subcommand", "new CLI command", "add a new cx command", "create a command", "add subcommand", "implement a new command", "build a cx command", "wire up a new command", "extend the CLI", "add an API to cx", "new cx feature", "integrate a Coralogix API", or wants to add new functionality to the cx CLI. Use this even when the user describes a feature that implies a new command without saying "command" explicitly.
version: 0.1.0
metadata:
  internal: true
---

# Add a CLI Command

End-to-end workflow for adding a new command to `cx`. Every command falls into one of two archetypes - determine which one first, then follow the corresponding steps.

`docs/adding-a-command.md` has copy-pasteable code templates for every step below. Read it alongside this workflow.

## Step 0: Understand What You're Building

Before writing any code, get clarity on the domain:

1. **What Coralogix API are you wrapping?** Find the API docs or example responses. Understand the data model - what entities exist, what fields they have, what operations are supported.
2. **What should the user be able to do?** List the subcommands (e.g., `list`, `get`, `create`) and what flags make sense.
3. **Does this belong under a wrapper group?** The CLI organizes related commands into wrapper groups. Check if your command fits under an existing group before creating a top-level command:
   - `cx alerts` - alert definitions + `schedulers`
   - `cx notifications` - `connectors`, `routers`, `presets`, `test`
   - `cx webhooks` - outgoing webhooks + `actions`
   - `cx enrichments` - enrichment rules + `custom` enrichment tables
   - `cx integrations` - integrations + `extensions`, `contextual-data`
   - `cx iam` - `api-keys`, `roles`, `scopes`, `users`, `groups`, `ip-access`
   Run `cx schema` to see the full command tree as JSON.
4. **Which archetype fits?**

| Archetype | When to use | Reference implementation |
|-----------|------------|--------------------------|
| **A: DataPrime-based** | Querying logs, spans, or any DataPrime source | `src/commands/logs/mod.rs` |
| **B: REST-based** | Wrapping a Coralogix REST API (most new commands) | `src/commands/alerts/api.rs` + `src/commands/alerts/mod.rs` |

DataPrime commands delegate to a shared pipeline and require minimal code (~130 lines). REST commands build the full pipeline (API client, fan-out, merge, render) - more code but more control.

**Important:** All API integrations must use REST (HTTP). The CLI is HTTP-only by design - do not use gRPC.

## Step 1: Read Reference Implementations

Before writing any code, read these files to internalize the existing patterns. This step is critical - agents that read existing code first produce implementations that are consistent with the codebase rather than inventing new patterns.

**Always read:**
- `src/main.rs` - study the `Commands` enum to see how variants are structured, and the `match cli.command` dispatch block to see where your new variant fits. Note which commands early-exit (no credentials needed) vs which go through the full config resolution flow. Pay attention to **wrapper groups** (e.g., `Notifications`, `Iam`, `Webhooks`, `Integrations`) - these are top-level commands with nested subcommand enums that group related domains. If your command belongs under an existing group, add a new variant to that group's subcommand enum rather than creating a top-level command.
- `src/commands/mod.rs` - see existing module registrations so you add yours in the right place
- `docs/adding-a-command.md` - full guide with code templates for both archetypes

**DataPrime archetype - also read:**
- `src/commands/logs/mod.rs` - a complete DataPrime command; notice how little code is needed because the shared pipeline does the heavy lifting
- `src/commands/dataprime/mod.rs` - the shared pipeline your command will delegate to; understand the `run_query()` signature and what it handles (fan-out, merge, spilling, agents output)

**REST archetype - also read:**
- `src/commands/alerts/api.rs` - see how response types are structured, how the API struct borrows `&CxClient`, how deserialization tests are written
- `src/commands/alerts/mod.rs` - see how the handler declares `pub mod api;` and imports types via `use api::{...};`
- `src/commands/dashboards/mod.rs` - see the fan-out/merge/render pattern using `render::*` helpers, and how all three output formats are handled

## Step 2: Create API Layer (REST Only)

> Skip this step for DataPrime commands - they use the shared DataPrime pipeline.

Create `src/commands/<domain>/api.rs`. See `docs/adding-a-command.md` § "Archetype B, Step 1" for the full template.

Key conventions and why they matter:

- **`#[serde(rename_all = "camelCase")]`** on response types - Coralogix APIs use camelCase JSON keys
- **`#[serde(default)]` on `Vec` fields** - the API sometimes omits empty arrays entirely rather than sending `[]`, so this prevents deserialization failures
- **`Option<T>` for fields that may be absent** - be defensive, APIs evolve and fields vary across tiers
- **API struct borrows `&CxClient`** (don't own it) - the client is shared across the fan-out and must outlive individual API calls
- **`const BASE_PATH`** for the endpoint prefix - keeps URLs DRY
- **Deserialization tests are mandatory** - test both happy-path and edge cases (empty lists, missing optional fields) since these are the cases that break in production

## Step 3: Create Command Module

Create `src/commands/<domain>/mod.rs`. For REST commands, declare `pub mod api;` at the top so the handler can `use api::{...};` types from its sibling `api.rs`. See `docs/adding-a-command.md` for full templates of both archetypes.

### DataPrime archetype

Provide two things:
1. A text renderer: `pub fn render_<domain>_text(merged: &MergedResults) -> Result<()>` - called only for `OutputFormat::Text`; JSON and Agents output are handled by the shared pipeline
2. A thin `run()` wrapper that calls `super::dataprime::run_query()` with your DataPrime source name

### REST archetype

Build the full fan-out/merge/render pipeline. Key patterns to understand:

- **`render::render_table`** for text output - pass column headers (without "Profile") and rows where the first element is the profile name. The helper conditionally includes the Profile column based on `include_profile`. No duplicate struct definitions needed.
- **`render::render_json`** for JSON output - pretty-prints a `&[Value]` array
- **`let include_profile = targets.len() > 1;`** - this single boolean controls all multi-profile behavior (Profile column in text, `"profile"` key in JSON)
- **Fan-out errors are non-fatal** - print to stderr and continue, because one misconfigured profile shouldn't block results from others
- **Status messages go to stderr** (`eprintln!`) - stdout is reserved for data so piped output isn't polluted
- **Agents output is command-owned** - each command calls `toon_encode` directly after any post-processing, because different commands may transform data differently before encoding

Register the module in `src/commands/mod.rs`.

## Step 4: Wire into CLI

In `src/main.rs`, add three things. See `docs/adding-a-command.md` § "CLI Wiring" for templates.

1. **`Commands` enum variant** - DataPrime commands use inline args; REST commands reference a subcommand enum
2. **Subcommand enum** (REST only) - defines `List`, `Get`, etc.
3. **Dispatch match arm** - inside the `match cli.command` block. Most commands go through the full config resolution flow; only commands that don't need credentials (like `profiles`, `cleanup`) early-exit.

## Step 5: Add Tests

Every new command must add tests at three layers. See
`docs/adding-a-command.md` § "Testing" for code templates and examples
of each.

| Layer | Location | What it verifies |
|-------|----------|------------------|
| Unit | `src/**/<file>.rs` `#[cfg(test)]` | Pure logic - deserialization (mandatory for REST), helpers, transforms |
| Integration | `tests/<command>/main.rs` (wiremock) | Command runner end-to-end with mocked HTTP |
| E2E | `tests/e2e/<command>/mod.rs` (assert_cmd, `#[ignore]`d) | Real `cx` binary against the Coralogix test team |

Things specific to *this workflow* that the doc doesn't emphasise:

- **Don't add e2e for mutating commands** (create/delete/enable/disable)
  unless there's a paired-undo plan - they touch shared test team state.
  Mark them as deliberately uncovered with a comment, like
  `tests/e2e/alerts/mod.rs`.
- **If a subcommand needs an ID from the test team** (e.g. `get <id>`),
  add a local `discover_*` fn in your e2e test module, modelled after
  `discover_alert_id` in `tests/e2e/alerts/mod.rs`. Cache via `OnceLock`
  and skip gracefully when the test team has no data - don't panic.
- **Don't forget to declare the new e2e module** in `tests/e2e.rs` via
  `#[path = "e2e/your_domain/mod.rs"] mod your_domain;`.

## Step 6: Create User-Facing Skill

Every command needs a corresponding skill in `skills/` so AI agents know how to use it. Use the **add-skill** workflow to create it - it walks through the full process including reading reference implementations, writing effective trigger descriptions, and verification.

## Step 7: Verify

Run `cargo build`, `cargo test` (unit + integration), `cargo clippy`,
and `cargo fmt --check`. Fix any issues before committing.

If you have test team credentials configured, also run the e2e suite:
```bash
cargo test --test e2e -- --ignored --test-threads=1
```

Smoke test all three output formats (`text`, `json`, `agents`) and
multi-profile (`-p profile1 -p profile2`).

See `docs/adding-a-command.md` § "PR Checklist" for the full checklist to include in your PR description.
