# Plan: Dashboard Replace Command

| Field | Value |
|-------|-------|
| Status | in-progress |
| Created | 2026-05-07 |
| Ticket | N/A |
| Branch | feat/dashboard-replace |

## Context

The Coralogix Dashboard Service exposes a PUT endpoint (`/mgmt/openapi/5/dashboards/dashboards/v1`) that replaces an existing dashboard with a new definition. The `cx dashboards` command currently supports catalog, get, create, delete, and folders - but has no way to update an existing dashboard. This means the common workflow of "get dashboard JSON, edit it, push it back" is impossible without the UI. Adding `replace` closes this gap.

## Architecture Decisions

- **Command name: `replace`** - Mirrors the API name ("Replace a Dashboard") and matches `slos` which uses `api.replace()` internally. "update" would also work but "replace" makes it clear this is a full replacement, not a patch.
- **No dashboard ID argument** - The dashboard ID is embedded in the JSON body (inside `dashboard.id`). The API uses PUT to the base path, not `PUT /{id}`. This matches how `create` works (body-only, no positional ID). The user gets the JSON via `cx dashboards get <id>`, edits it, and passes it back.
- **Reuse `read_dashboard_body()`** - The existing helper validates JSON structure (requires `layout` field, accepts bare or wrapped form). For replace, we also validate that `id` is present in the dashboard object.
- **Follow SLOs pattern** - `slos update` is the closest existing command: reads JSON from file, calls `api.replace()` via PUT, renders result. Dashboard replace follows the same structure.

## Milestones Overview

1. **Dashboard Replace Command** - Users can update existing dashboards from the CLI via `cx dashboards replace --from-file`
2. **Skill & Documentation Updates** - The cx-create-dashboard skill knows about replace and can use it for iterative dashboard development

---

## Milestone 1: Dashboard Replace Command

**Why this matters:** Users who manage dashboards as code (or iterate on dashboard JSON) currently have to use the Coralogix UI to update an existing dashboard. With `replace`, they can `cx dashboards get <id> -o json > dash.json`, edit the file, and `cx dashboards replace --from-file dash.json` - a fully CLI-native workflow.

**Success criteria:** A user can round-trip a dashboard: get it as JSON, modify a widget title, replace it, and verify the change took effect with another get.

**Key decisions:** No positional `dashboard_id` arg - the ID comes from the JSON body, matching the PUT API contract. The `--from-file` flag (defaulting to stdin) matches `create`.

### Deliverable Spec

| Command | Required args | Optional args | Description |
|---------|--------------|---------------|-------------|
| `cx dashboards replace --from-file <path>` | `--from-file` (or stdin) | `--yes`, `-o` | Replace an existing dashboard with updated JSON |

### 1.1 [x] Add `replace` API method *(completed 2026-05-07)*
- **Files:** `src/commands/dashboards/api.rs`
- **What:** Add a `replace(&self, body: &Value) -> Result<Value>` method to `DashboardsApi` that sends a PUT request to `DASHBOARDS_BASE`. The body is the full `{ "requestId": ..., "dashboard": { ... } }` envelope, same shape as create. Follow the pattern used by `slos/api.rs::replace()`.
- **Acceptance:** `cargo build` succeeds. Unit test deserializes a mock PUT response.
- **Dependencies:** None

### 1.2 [x] Add `replace` command handler *(completed 2026-05-07)*
- **Files:** `src/commands/dashboards/mod.rs`
- **What:** Add `run_replace(targets, from_file, output)` async function. It should: (1) call `read_dashboard_body()` to parse and validate the JSON, (2) verify the dashboard object contains an `id` field (error if missing - can't replace without knowing which dashboard), (3) wrap in `{ "requestId": new_request_id(), "dashboard": ... }` envelope, (4) fan out across targets calling `api.replace()`, (5) render results the same way `run_create` does (extract ID and name from response, print confirmation). The response extraction logic (looking for ID in `dashboardId`, `id`, or `dashboard.id`) can be shared with or copied from `run_create`.
- **Acceptance:** `cargo build` succeeds. `cargo clippy` clean.
- **Dependencies:** 1.1

### 1.3 [x] Wire up CLI subcommand and dispatcher *(completed 2026-05-07)*
- **Files:** `src/main.rs`
- **What:** Add a `Replace` variant to the `DashboardsCmd` enum with `--from-file` arg (default `"-"` for stdin), matching `Create`'s pattern. Add an after_help with examples showing the get-edit-replace workflow. In the `Commands::Dashboards` match arm, add the `Replace` case that calls `confirm_destructive("Replace dashboard?", yes, agent_mode)?` then `commands::dashboards::run_replace()`. Add helpful examples in the after_help block.
- **Acceptance:** `cx dashboards replace --help` shows the command with examples. `cx dashboards --help` lists `replace` as a subcommand. `cargo test` passes (no regressions). `cargo clippy` clean.
- **Dependencies:** 1.2

### 1.4 [ ] Add integration tests (wiremock)
- **Files:** `tests/dashboards/main.rs`
- **What:** Add a wiremock test for the replace command. Mock a PUT to the dashboards endpoint that returns a dashboard response. Run `run_replace` against it and verify the output. Follow the patterns in the existing `tests/dashboards/main.rs` for catalog/get/delete. Test both JSON and text output modes.
- **Acceptance:** `cargo test --test dashboards` passes with the new test.
- **Dependencies:** 1.3

### 1.5 [ ] Add E2E test
- **Files:** `tests/e2e/dashboards/mod.rs`
- **What:** Add an `#[ignore]`d E2E test that does a round-trip: (1) create a dashboard from a fixture, (2) get it as JSON, (3) modify the name, (4) replace it, (5) get again and verify the name changed, (6) delete it (cleanup). Use the existing `harness` helpers. This test is gated on `CX_API_KEY`.
- **Acceptance:** `cargo test --test e2e -- --ignored dashboards_replace --test-threads=1` passes against the test environment.
- **Dependencies:** 1.3

---

## Milestone 2: Skill & Documentation Updates

**Why this matters:** The `cx-create-dashboard` skill drives AI-assisted dashboard creation. Without knowing about `replace`, the skill can only create new dashboards - it can't iterate on existing ones. Adding replace awareness enables a "get, modify, replace" workflow that's essential for dashboard refinement.

**Success criteria:** An agent using the cx-create-dashboard skill can update an existing dashboard without the user switching to the UI.

**Key decisions:** Update the deploy reference doc (Phase 7) to include a "replace existing" path alongside "create new". Update SKILL.md trigger description to include update/modify/replace keywords.

### Before/After

Currently the cx-create-dashboard skill only knows how to create new dashboards via `cx dashboards create`. After this milestone, the skill also documents the replace workflow and triggers on "update dashboard" / "modify dashboard" / "replace dashboard" requests.

### 2.1 [ ] Update deploy reference and SKILL.md
- **Files:** `skills/cx-create-dashboard/references/deploy.md`, `skills/cx-create-dashboard/SKILL.md`
- **What:** In `deploy.md`, add a new section (after the existing create flow) documenting the replace workflow: `cx dashboards get <id> -o json > dash.json`, edit, `cx dashboards replace --from-file dash.json --yes`. In `SKILL.md`, add "replace", "update", "modify" to the trigger description so the skill activates on dashboard update requests. Add the `replace` command to whatever command reference table exists in the skill.
- **Acceptance:** Skill triggers on "update my dashboard" and "replace dashboard" queries. The deploy reference includes both create-new and replace-existing paths.
- **Dependencies:** 1.3

### 2.2 [ ] Final build verification
- **Files:** None (verification only)
- **What:** Run `cargo install --path .` to do a full release-profile build and install. Verify `cx dashboards --help` shows `replace`. Verify `cx dashboards replace --help` shows the expected flags and examples.
- **Acceptance:** `cargo install --path .` succeeds. `cx dashboards replace --help` output is correct.
- **Dependencies:** 2.1
