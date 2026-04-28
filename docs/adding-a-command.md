# Adding a Command

> Step-by-step guide for adding a new command to `cx`. Read [architecture.md](architecture.md) first for the execution flow and design decisions behind this structure.

## Choose your archetype

Every command falls into one of two patterns:

| Archetype | When to use | Reference implementation |
|-----------|------------|--------------------------|
| **A: DataPrime-based** | Querying logs, spans, or any DataPrime source | `logs` (`src/commands/logs.rs`) |
| **B: REST-based** | Wrapping a Coralogix REST API | `alerts` (`src/commands/alerts.rs`, `src/api/alerts.rs`) |

> **Important:** All API integrations must use REST (HTTP). Do not use gRPC — the CLI is HTTP-only by design.

DataPrime commands delegate to the shared pipeline and require minimal code. REST commands manage their own fan-out, merge, and render — more code, but more control.

---

## Archetype A: DataPrime-based command

Use this when your command queries a DataPrime source. The shared pipeline in `commands::dataprime` handles fan-out, merge, render, and spilling — you provide only a text renderer and a thin `run()` wrapper.

**Files to create/modify:**
- `src/commands/your_domain.rs` (new)
- `src/commands/mod.rs` (add module)
- `src/main.rs` (CLI definition + dispatch)

### Step 1: Command module

Create `src/commands/your_domain.rs`:

```rust
use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;

use crate::commands::dataprime::MergedResults;
use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::Tier;

// ── Text renderer ──────────────────────────────────────────────────

pub fn render_your_domain_text(merged: &MergedResults) -> Result<()> {
    if merged.rows.is_empty() {
        println!("{}", "No results found.".yellow());
        return Ok(());
    }

    // Aggregate queries return raw JSON — no custom rendering.
    if merged.is_aggregate {
        for row in &merged.rows {
            println!("{}", serde_json::to_string_pretty(row)?);
        }
        return Ok(());
    }

    for row in &merged.rows {
        // Multi-profile: prefix each line with [profile_name]
        let profile = if merged.include_profile {
            row.get("profile")
                .and_then(|v| v.as_str())
                .map(|s| format!("[{s}] "))
                .unwrap_or_default()
        } else {
            String::new()
        };

        // Extract your domain-specific fields from the JSON row.
        // Use row.pointer("/path/to/field") for nested access.
        let field_a = row.pointer("/metadata/some_field")
            .and_then(|v| v.as_str())
            .unwrap_or("-");

        println!("{}{}", profile.dimmed(), field_a);
    }

    Ok(())
}

// ── Orchestrator ───────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run(
    targets: &[Arc<ExecutionTarget>],
    query: &str,
    start: &str,
    end: &str,
    limit: u32,
    tier: Tier,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
) -> Result<()> {
    super::dataprime::run_query(
        targets,
        query,
        "your_source",  // DataPrime source name (e.g., "logs", "spans")
        start,
        end,
        limit,
        tier,
        output,
        max_direct,
        temp_dir,
        Some(render_your_domain_text),
    )
    .await
}
```

The text renderer signature must be `fn(&MergedResults) -> Result<()>`. The shared pipeline calls it only for `OutputFormat::Text` — JSON and Agents output are handled generically.

**Reference:** `src/commands/logs.rs` — the entire module is ~130 lines.

### Step 2: Register the module

Add your module to `src/commands/mod.rs`:

```rust
pub mod your_domain;
```

### Step 3: CLI wiring

In `src/main.rs`, add a variant to the `Commands` enum:

```rust
#[derive(Subcommand)]
enum Commands {
    // ... existing variants ...

    /// Query your-domain data using DataPrime syntax.
    #[command(after_help = "\
Examples:
  cx your-domain 'filter $d.field == \"value\"'
  cx your-domain 'filter $m.severity == ERROR' --start now-6h")]
    YourDomain {
        /// DataPrime query string.
        query: String,

        #[arg(long, default_value = "now-1h")]
        start: String,

        #[arg(long, default_value = "now")]
        end: String,

        #[arg(long, default_value_t = 100)]
        limit: u32,

        #[arg(long, default_value = "frequent")]
        tier: Tier,
    },
}
```

Add the dispatch match arm (after config resolution, inside the `match cli.command` block):

```rust
Commands::YourDomain {
    query,
    start,
    end,
    limit,
    tier,
} => {
    commands::your_domain::run(
        &targets, &query, &start, &end, limit, tier, output, max_direct, &temp_dir,
    )
    .await?;
}
```

That's it for a DataPrime command. The shared pipeline handles fan-out, merge, agents output, and spilling.

---

## Archetype B: REST-based command

Use this when your command wraps a Coralogix REST API. You'll build the full pipeline: API client, fan-out, merge, and render.

**Files to create/modify:**
- `src/api/your_domain.rs` (new — API types + client)
- `src/api/mod.rs` (add module)
- `src/commands/your_domain.rs` (new — fan-out, merge, render)
- `src/commands/mod.rs` (add module)
- `src/main.rs` (CLI definition + dispatch)

### Step 1: API module

Create `src/api/your_domain.rs`:

```rust
use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// ── Response types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YourItem {
    pub id: Option<String>,
    pub name: Option<String>,
    // Add fields matching the API's JSON response.
    // Use Option<T> for fields that may be absent.
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResponse {
    #[serde(default)]
    pub items: Vec<YourItem>,
}

// ── API client ─────────────────────────────────────────────────────

const BASE_PATH: &str = "/mgmt/openapi/latest/your-domain/v1";

pub struct YourDomainApi<'a> {
    client: &'a CxClient,
}

impl<'a> YourDomainApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListResponse> {
        self.client.get(BASE_PATH, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{BASE_PATH}/{id}");
        self.client.get(&path, &[]).await
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_list_response() {
        let json = json!({
            "items": [
                { "id": "abc-123", "name": "First Item" },
                { "id": "def-456", "name": "Second Item" }
            ]
        });
        let resp: ListResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.items.len(), 2);
        assert_eq!(resp.items[0].id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListResponse = serde_json::from_value(json).unwrap();
        assert!(resp.items.is_empty());
    }
}
```

Key conventions:
- Response types derive `Deserialize` with `#[serde(rename_all = "camelCase")]`
- Use `#[serde(default)]` on Vec fields so missing keys deserialize to empty
- The API struct borrows `&CxClient` — use the appropriate method (`get`, `post`, `post_raw`, `post_empty`) based on the endpoint
- Always write deserialization tests against realistic JSON fixtures

**Reference:** `src/api/alerts.rs` — full example with list, get, create, and state-change endpoints.

### Step 2: Register the API module

Add to `src/api/mod.rs`:

```rust
pub mod your_domain;
```

### Step 3: Command module

Create `src/commands/your_domain.rs`:

```rust
use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::api::your_domain::YourDomainApi;
use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;

// ── Subcommand runner ──────────────────────────────────────────────

pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching items...".dimmed());

    let include_profile = targets.len() > 1;

    // 1. Fan-out: call the API across all profiles concurrently.
    let per_profile = fan_out(targets, |t| async move {
        let api = YourDomainApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    // 2. Merge: collect results, print per-profile errors to stderr.
    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, String, String)> = Vec::new(); // (profile, id, name)
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for item in resp.items {
                    let id = item.id.clone().unwrap_or_default();
                    let name = item.name.clone().unwrap_or_default();
                    let mut j = json!({ "id": id, "name": name });
                    if include_profile {
                        j.as_object_mut().unwrap()
                            .insert("profile".to_string(), Value::String(profile.clone()));
                    }
                    all_json.push(j);
                    all_items.push((profile.clone(), id, name));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    // 3. Render: match on output format.
    match output {
        OutputFormat::Json => render::render_json(&all_json)?,
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                render::print_no_results("No items found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, id, name)| {
                    vec![profile.clone(), id.clone(), name.clone()]
                })
                .collect();
            render::render_table(&["ID", "Name"], rows, include_profile);
        }
    }

    Ok(())
}
```

Key patterns to follow:
- **`include_profile = targets.len() > 1`** — this boolean controls all multi-profile behavior
- **`render::render_table`** handles the Profile column automatically — pass headers without "Profile", and put the profile name as the first element of each row. The helper conditionally includes/excludes it based on `include_profile`.
- **Agents output is command-owned** — each command calls `toon_encode` directly after any post-processing it needs
- **Fan-out errors are non-fatal** — print to stderr, continue with successful profiles
- **Status messages go to stderr** — use `eprintln!` so they don't pollute piped output

**Reference:** `src/commands/dashboards.rs` — clean example with list and get subcommands using `render::*` helpers.

### Step 4: Register the command module

Add to `src/commands/mod.rs`:

```rust
pub mod your_domain;
```

### Step 5: CLI wiring

In `src/main.rs`, define the subcommand enum and add to `Commands`:

```rust
#[derive(Subcommand)]
enum YourDomainCmd {
    /// List all items.
    List,
    /// Get a single item by ID.
    Get {
        /// Item ID.
        item_id: String,
    },
}

#[derive(Subcommand)]
enum Commands {
    // ... existing variants ...

    /// Manage your-domain resources.
    YourDomain {
        #[command(subcommand)]
        cmd: YourDomainCmd,
    },
}
```

Add the dispatch match arm:

```rust
Commands::YourDomain { cmd } => match cmd {
    YourDomainCmd::List => {
        commands::your_domain::run_list(&targets, output).await?;
    }
    YourDomainCmd::Get { item_id } => {
        commands::your_domain::run_get(&targets, &item_id, output).await?;
    }
},
```

### Step 6: Update the help display

The `Cli` struct uses a custom `after_help` string (not Clap's `next_help_heading` or `flatten`) to display grouped command categories. After adding a command to the `Commands` enum, add a line for it in the `after_help` string under the appropriate category group:

```rust
#[command(
    help_template = "{about-with-newline}\nUsage: {usage}{after-help}\n\nOptions:\n{options}",
    after_help = "\
Query:
  logs               Query logs using DataPrime syntax
  ...
Data Pipeline:
  rules              Manage log parsing rule groups
  your-domain        Your domain description        <-- add here
  ..."
)]
```

Place the command in the category that best fits its purpose. See `main.rs` for the full list of categories: Query, Observe, Detect & Respond, Notifications, Data Pipeline, Cost & Storage, Integrations, Access, Agent, Local.

---

## Adding a subcommand to a wrapper command

Some commands group multiple related domains under a single CLI entry point using a **wrapper enum**. Examples: `iam` (api-keys, roles, scopes, users, groups, saml, ip-access), `notifications` (connectors, routers, presets, test), `integrations` (extensions, contextual-data).

If your new functionality belongs under an existing wrapper, you do not create a new top-level command. Instead:

1. **Create your API and command modules** as usual (`src/api/your_sub.rs`, `src/commands/your_sub.rs`)
2. **Add a variant to the wrapper enum** (e.g., `IamCmd`, `NotificationsCmd`) in `main.rs`:

```rust
#[derive(Subcommand)]
enum IamCmd {
    // ... existing variants ...

    /// Manage your-sub resources.
    YourSub {
        #[command(subcommand)]
        cmd: YourSubCmd,
    },
}
```

3. **Add the leaf subcommand enum** (`YourSubCmd`) with its operations (list, get, etc.)
4. **Add dispatch** in the existing wrapper's match arm
5. **Update the wrapper's `after_help`** in the `Commands` enum variant to include the new sub-domain in its examples

The wrapper variant in `Commands` already appears in the top-level `after_help`, so no change is needed there unless the wrapper's description should be updated.

**Reference:** See `IamCmd` in `main.rs` for a wrapper with seven sub-domains, or `NotificationsCmd` for a wrapper with four.

---

## Testing

Every new command must add tests at three layers: **unit**, **integration**,
and **e2e**. Each layer catches different categories of regressions —
skipping any of them leaves real holes.

| Layer | Location | What it verifies | Network |
|-------|----------|------------------|---------|
| Unit | `src/**/<file>.rs` `#[cfg(test)]` blocks | Pure logic — deserialization, formatting helpers, data transforms | None |
| Integration | `tests/<domain>.rs` (wiremock) | Command runner end-to-end with mocked HTTP responses | None |
| E2E | `tests/e2e/<domain>.rs` (assert_cmd) | Real `cx` binary runs against the Coralogix test team | Real |

### Layer 1 — Unit tests

#### Deserialization tests (REST archetype, mandatory)

Every API module must have deserialization tests. These verify that your
response types correctly parse the actual API JSON shape.

```rust
// in src/api/your_domain.rs
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_list_response() {
        let json = json!({
            "items": [
                { "id": "abc-123", "name": "Test Item" }
            ]
        });
        let resp: ListResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.items.len(), 1);
    }

    #[test]
    fn deserialize_empty_response() {
        let json = json!({});
        let resp: ListResponse = serde_json::from_value(json).unwrap();
        assert!(resp.items.is_empty());
    }
}
```

Test happy-path responses **and** edge cases: empty lists, missing optional
fields, fallback values. These are the cases that break in production.

#### Helper/formatting tests (any archetype, when applicable)

If your command module has non-trivial mapping logic — building tabular
rows, transforming JSON shapes, parsing user input — add unit tests for
those helpers. See `src/commands/metrics/tests.rs` for an example.

### Layer 2 — Integration tests (wiremock)

Add `tests/<your_domain>.rs` using `wiremock` to spin up a fake Coralogix
API and call your command runner directly. This catches regressions in
fan-out, merge, rendering, and the wiring between command and API layer.

Reference: `tests/alerts.rs`, `tests/metrics.rs`, `tests/search_fields.rs`.

```rust
// tests/your_domain.rs
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::your_domain::run_list;
use cx::config::OutputFormat;

#[tokio::test]
async fn list_returns_items_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/your-domain/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": "abc-123", "name": "Test" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list(&targets, OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}
```

Cover at minimum: happy-path list/get, an empty response, a `--name`/filter
case if your command supports one, and the JSON output path.

### Layer 3 — E2E tests (real test team)

Add a sanity test in `tests/e2e/<your_domain>.rs` that invokes the
compiled `cx` binary against a real Coralogix test team. The goal is
**only** to verify that the command runs end-to-end: exits 0, produces
non-empty stdout, and (for `-o json`) emits valid JSON. Don't assert on
output content — test team data drifts.

All e2e tests are `#[ignore]`d, so they don't run in the default
`cargo test`. CI invokes them via a separate workflow.

```rust
// tests/e2e/your_domain.rs
use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn your_domain_list() {
    if harness::require_creds("your_domain_list").is_none() {
        return;
    }
    harness::run_ok_json(&["your-domain", "list", "-o", "json"]);
}

#[test]
#[ignore]
fn your_domain_get() {
    if harness::require_creds("your_domain_get").is_none() {
        return;
    }
    let Some(id) = discover_your_domain_id() else {
        eprintln!("[e2e] skipping your_domain_get: no items on test team");
        return;
    };
    harness::run_ok_json(&["your-domain", "get", &id, "-o", "json"]);
}

/// Discover an id from `your-domain list -o json`. Cached so multiple
/// tests don't each pay for the list call.
fn discover_your_domain_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["your-domain", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .filter_map(|item| item.get("id").and_then(|x| x.as_str()))
                .next()
                .map(String::from)
        })
        .clone()
}
```

Then declare the module in `tests/e2e.rs`:

```rust
#[path = "e2e/your_domain.rs"]
mod your_domain;
```

Discovery helpers stay local to each test module — see
`discover_alert_id` in `tests/e2e/alerts.rs` for the pattern. They
should cache via `OnceLock` and skip (return `None`) when the test team
has no data, not panic.

**Do not exercise mutating commands** in e2e (create/delete/enable/
disable) until there's a paired-undo plan — they touch shared test team
state. Use a comment to mark them as deliberately uncovered, like the
existing block at the bottom of `tests/e2e/alerts.rs`.

### Running the suites

```bash
cargo test                                              # unit + integration
cargo test --test e2e -- --ignored --test-threads=1     # e2e (needs CX_API_KEY)
cargo clippy                                            # lint
cargo fmt --check                                       # format check
```

See [development.md](development.md) for the full e2e setup.

### Manual smoke testing

After building (`cargo build`), do at least one human-in-the-loop pass:

```bash
# DataPrime commands:
cx your-domain 'filter $d.field == "value"'
cx your-domain 'filter $d.field == "value"' -o json
cx your-domain 'filter $d.field == "value"' -o agents

# REST commands:
cx your-domain list
cx your-domain list -o json
cx your-domain get <id>

# Multi-profile:
cx -p prod -p staging your-domain list
```

---

## User-facing skill (required)

Every command must be covered by a skill in `skills/`. This can be a dedicated skill for the command, or a workflow skill that covers multiple related commands (e.g., `cx-cost-optimization` covers `cx usage`, `cx tco`, `cx quotas`, `cx retentions`, and `cx archive`). Check the existing workflow skills before creating a new one — your command may already be covered.

See **[Adding a Skill](adding-a-skill.md)** for the complete guide covering directory structure, frontmatter conventions, trigger phrases, reference files, and templates for both single-command and workflow skills.

**Reference:** `skills/cx-alerts/SKILL.md` (single-command) and `skills/cx-cost-optimization/SKILL.md` (workflow skill) — full examples.

---

## PR checklist

Copy this into your PR description:

```markdown
## Checklist

### API layer (REST archetype only)
- [ ] `src/api/your_domain.rs` — response types with `#[derive(Deserialize)]`
- [ ] `src/api/your_domain.rs` — `YourDomainApi` struct with methods
- [ ] `src/api/your_domain.rs` — deserialization tests for all response types
- [ ] `src/api/mod.rs` — module registered

### Command layer
- [ ] `src/commands/your_domain.rs` — subcommand runner(s) with fan-out/merge/render
- [ ] `src/commands/your_domain.rs` — dual row structs for text output (multi-profile + single)
- [ ] `src/commands/your_domain.rs` — all three output formats handled (Text, Json, Agents)
- [ ] `src/commands/mod.rs` — module registered

### CLI wiring
- [ ] `src/main.rs` — `Commands` enum variant added
- [ ] `src/main.rs` — subcommand enum added (REST) or args defined (DataPrime)
- [ ] `src/main.rs` — dispatch match arm added

### Tests
- [ ] **Unit:** API deserialization tests in `src/api/your_domain.rs` (REST)
- [ ] **Unit:** helper/formatting tests if the command module has non-trivial logic
- [ ] **Integration:** `tests/your_domain.rs` covering happy-path, empty response, and any filters via wiremock
- [ ] **E2E:** sanity test(s) in `tests/e2e/your_domain.rs`, declared in `tests/e2e.rs` via `#[path]`
- [ ] **E2E:** local `discover_*` fn added to `tests/e2e/<your_domain>.rs` if a subcommand needs an ID/name from the test team

### User-facing skill
- [ ] Command is covered by a skill in `skills/` (new or existing workflow skill)
- [ ] `scripts/verify-skills.sh` — all skills pass

### Verification
- [ ] `cargo build` succeeds
- [ ] `cargo test` passes (unit + integration)
- [ ] `cargo test --test e2e -- --ignored --test-threads=1` passes against the test team
- [ ] `cargo clippy` clean
- [ ] `cargo fmt --check` clean
- [ ] Manual smoke test: text, json, and agents output
- [ ] Manual smoke test: multi-profile (if applicable)
```
