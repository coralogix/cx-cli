# Plan: Add Confirmation Prompts to Risky CLI Commands (IAM & Archive)

| Field | Value |
|-------|-------|
| Status | in-progress |
| Created | 2026-04-29 |
| Ticket | N/A |
| Branch | liranhason/age-813-add-all-cx-apis-to-cli |

## Context

The `cx iam` and `cx archive` commands expose destructive write operations (create/update/delete API keys, modify SAML/SSO, change IP whitelisting, alter archive configuration). Rather than hiding these commands entirely, we'll add confirmation prompts to all write/mutate operations. Read-only operations (list, get, search) remain unguarded. This protects users and agents from accidental destructive actions while keeping the commands fully discoverable and usable.

## Architecture Decisions

- **Confirmation prompts on write ops, not hiding:** Users can still discover and read data freely. Only create/update/delete/enable/disable/set operations require confirmation.
- **Reuse existing `inquire::Confirm` pattern:** Already used in `src/commands/profiles/mod.rs:212` with `inquire = "0.9"` (already in Cargo.toml).
- **`--yes` flag to skip confirmation:** Add a global `--yes` flag to `Cli` struct for scripting. Agents must explicitly pass `--yes` to bypass the prompt; non-interactive stdin (piped) will cause the prompt to fail, which is the desired fail-safe.
- **Shared confirm helper in `src/confirm.rs`:** A single `confirm_destructive(action: &str, yes: bool) -> Result<()>` function avoids duplicating prompt logic across 27 match arms. Reference implementation: `src/commands/profiles/mod.rs:199-218`.
- **No changes to handler code:** The confirmation happens in `main.rs` match arms before calling the handler functions. Handler code in `src/commands/*/mod.rs` stays untouched.
- **Defense in depth for agents:** Two layers: (1) CLI prompt blocks non-interactive callers (hard gate), (2) skills instruct agents to always get user approval before passing `--yes`.

## Milestones Overview

1. **Add confirmation infrastructure** - Global `--yes` flag, shared confirm helper, guard all IAM + archive write ops
2. **Agent safety: update skills** - Add safety guidance to `cx-platform-admin` and `cx-cost-optimization` skills
3. **Test and verify** - All tests pass, E2E tests updated with `--yes`

---

## Milestone 1: Add Confirmation Infrastructure and Guard Write Operations

**Why this matters:** Prevents users and agents from accidentally running destructive IAM or archive operations. The confirmation prompt is the hard safety gate.

**Success criteria:** `cargo build` succeeds. All IAM and archive write operations prompt for confirmation. Read operations do NOT prompt. `--yes` bypasses the prompt. `cargo install --path .` installs the updated binary.

**Key decisions:** Global `--yes` flag on `Cli` struct (not per-subcommand) for simplicity. Guard at the match arm level in `main.rs`, not inside handler functions.

### Deliverable Spec

**Write operations that need confirmation (27 total):**

IAM - API Keys: `create`, `update`, `delete`, `admin delete`, `admin set-status`
IAM - Roles: `create`, `update`, `delete`
IAM - Scopes: `create`, `update`, `delete`
IAM - Users: `create`, `update`, `set-status`
IAM - Groups: `create`, `update`, `delete`
IAM - SAML: `set-idp`, `set-active`
IAM - IP Access: `create`, `update`, `delete`
Archive - Metrics: `create`, `update`, `enable`, `disable`
Archive - Logs: `set`

**Read operations that must NOT prompt:**

IAM: `list`, `get`, `search`, `system`, `sp-params`, `send-data-keys`, `get-by-name`, `users` (group members), `admin list`
Archive: `get`, `validate`

### 1.1 [x] Add global --yes flag and confirm helper module *(completed 2026-04-29)*

- **Files:** `src/main.rs`, `src/confirm.rs` (new), `src/lib.rs`
- **What:**
  1. Create `src/confirm.rs` with:
     ```rust
     use anyhow::{bail, Result};
     use inquire::Confirm;

     pub fn confirm_destructive(action: &str, yes: bool) -> Result<()> {
         if yes {
             return Ok(());
         }
         let confirmed = Confirm::new(action).with_default(false).prompt()?;
         if !confirmed {
             bail!("Cancelled.");
         }
         Ok(())
     }
     ```
  2. Register the module in `src/lib.rs` (line 1-area): add `pub mod confirm;`
  3. In `src/main.rs`, add `use coralogix_cli::confirm::confirm_destructive;` to the imports
  4. Add `--yes` flag to the `Cli` struct (~line 91):
     ```rust
     /// Skip confirmation prompts for destructive operations.
     #[arg(long, global = true)]
     yes: bool,
     ```
  5. Extract `yes` before the match statement (~line 2128, before `match cli.command`): `let yes = cli.yes;`
- **Acceptance:** `cargo build` succeeds. `cx --help` shows `--yes` flag under options. No behavioral changes yet.
- **Dependencies:** None

### 1.2 [x] Add confirmation prompts to all IAM and archive write operations *(completed 2026-04-29)*

- **Files:** `src/main.rs`
- **What:** In the `Commands::Iam` match arm (~lines 2790-2939) and `Commands::DataArchive` match arm (~lines 2942-2974), add a `confirm_destructive("...", yes)?;` call BEFORE each write operation's handler invocation.

  **Pattern** - for each write match arm, insert one line before the handler call:
  ```rust
  ApiKeysCmd::Create { from_file } => {
      confirm_destructive("Create a new API key?", yes)?;      // <-- add this
      commands::api_keys::run_create(&targets, &from_file, output).await?;
  }
  ```

  **Confirmation messages for each write operation:**

  IAM - API Keys:
  - `Create { from_file }` -> `"Create a new API key?"`
  - `Update { from_file, id }` -> `&format!("Update API key '{id}'?")`
  - `Delete { id }` -> `&format!("Delete API key '{id}'?")`
  - `Admin > Delete { ids }` -> `"Bulk delete API keys?"`
  - `Admin > SetStatus { ids, active }` -> `&format!("Set API key status to active={active}?")`

  IAM - Roles:
  - `Create { from_file }` -> `"Create a new custom role?"`
  - `Update { from_file, id }` -> `&format!("Update role '{id}'?")`
  - `Delete { id }` -> `&format!("Delete role '{id}'?")`

  IAM - Scopes:
  - `Create { from_file }` -> `"Create a new scope?"`
  - `Update { from_file }` -> `"Update scope?"`
  - `Delete { id }` -> `&format!("Delete scope '{id}'?")`

  IAM - Users:
  - `Create { from_file }` -> `"Create user(s)?"`
  - `Update { from_file }` -> `"Update user(s)?"`
  - `SetStatus { user_ids, status }` -> `&format!("Set user status to '{status}'?")`

  IAM - Groups:
  - `Create { from_file }` -> `"Create a new team group?"`
  - `Update { from_file, id }` -> `&format!("Update team group '{id}'?")`
  - `Delete { id }` -> `&format!("Delete team group '{id}'?")`

  IAM - SAML:
  - `SetIdp { from_file }` -> `"Update SAML IDP configuration? This may affect SSO for all users."`
  - `SetActive { active }` -> `&format!("Set SAML active to {active}? This may affect SSO for all users.")`

  IAM - IP Access:
  - `Create { from_file }` -> `"Create IP access rules?"`
  - `Update { from_file }` -> `"Update IP access rules?"`
  - `Delete` -> `"Delete all IP access rules? This removes all IP restrictions."`

  Archive - Metrics:
  - `Create { from_file }` -> `"Create metrics archive configuration?"`
  - `Update { from_file }` -> `"Update metrics archive configuration?"`
  - `Enable` -> `"Enable metrics archiving?"`
  - `Disable` -> `"Disable metrics archiving?"`

  Archive - Logs:
  - `Set { from_file }` -> `"Set logs archive target?"`

  Do NOT add confirmation to any read-only operations.

- **Acceptance:** `cargo build` succeeds. `cargo clippy` passes. Test manually: `cargo run -- iam api-keys list` does NOT prompt. `cargo run -- iam api-keys create --from-file -` shows confirmation prompt. `cargo run -- iam api-keys create --from-file - --yes` skips prompt.
- **Dependencies:** 1.1

### 1.3 [x] Build and install *(completed 2026-04-29)*

- **Files:** None (build step)
- **What:** Run `cargo fmt`, `cargo clippy`, `cargo build`, and `cargo install --path .`
- **Acceptance:** All pass without errors or warnings. `cx --yes --help` works.
- **Dependencies:** 1.2

---

## Milestone 2: Agent Safety - Update Skills

**Why this matters:** The CLI prompt is the hard gate, but agents using skills need explicit guidance to ask the user before passing `--yes`. Without this, a well-meaning agent might add `--yes` to "make the command work" when the prompt blocks it.

**Success criteria:** Both affected skills contain prominent safety instructions about `--yes` and user confirmation.

**Key decisions:** Safety section goes early in the skill file (right after intro, before command tables) so agents see it before executing any commands.

### 2.1 [ ] Add safety instructions to cx-platform-admin skill

- **Files:** `skills/cx-platform-admin/SKILL.md`
- **What:** Add a `## Destructive Operation Safety` section right after the intro paragraph (line 17, before `## CLI Commands`). Content:

  ```markdown
  ## Destructive Operation Safety

  All write operations (create, update, delete, set-idp, set-active, set-status) require interactive confirmation. The CLI will prompt the user before executing. To skip the prompt in scripts, pass `--yes`.

  **IMPORTANT: NEVER pass `--yes` without explicit user approval.** Before executing any write operation:
  1. Describe the exact operation to the user (what will be created/modified/deleted)
  2. Wait for the user to confirm
  3. Only then execute with `--yes`

  Read-only operations (list, get, search, system, sp-params, send-data-keys) do not require confirmation and can be run freely.
  ```

  Also update the write command examples in the "API Key Rotation" section and elsewhere to include `--yes` where appropriate, showing the pattern of `--yes` usage.

- **Acceptance:** Skill file contains safety section before CLI commands. Write operation guidance is clear.
- **Dependencies:** 1.2

### 2.2 [ ] Add safety instructions to cx-cost-optimization skill

- **Files:** `skills/cx-cost-optimization/SKILL.md`
- **What:** Add a `## Destructive Operation Safety` section in the "Applying Changes" section (~line 179), since that's where the skill transitions from read to write operations. Content follows the same pattern as 2.1:

  ```markdown
  **IMPORTANT: NEVER pass `--yes` without explicit user approval.** Archive write operations (`cx archive logs set`, `cx archive metrics create/update/enable/disable`) require interactive confirmation. Before executing any write operation, describe the exact change to the user and wait for their approval before passing `--yes`.
  ```

  The rest of the cost commands (usage, tco, retentions, quotas) don't have CLI confirmation prompts in this plan, but the skill should still advise agents to confirm with users before modifying TCO policies, retention settings, or quotas since those are impactful changes.

- **Acceptance:** Skill file contains safety guidance in the applying-changes section.
- **Dependencies:** 1.2

---

## Milestone 3: Test and Verify

**Why this matters:** Ensures all existing tests still pass. Unit tests call handler functions directly (bypassing main.rs) so they should be unaffected. E2E tests invoke the binary and may hit the prompt on write operations.

**Success criteria:** `cargo fmt --check`, `cargo clippy`, and `cargo test` all pass with zero warnings. `cargo install --path .` succeeds.

### Before/After

Currently E2E tests for `cx iam` and `cx archive` run commands directly. After this milestone, any E2E tests that invoke write operations pass `--yes` to bypass the confirmation prompt.

### 3.1 [ ] Update E2E tests to pass --yes flag and run full suite

- **Files:** `tests/e2e/api_keys.rs`, `tests/e2e/roles.rs`, `tests/e2e/scopes.rs`, `tests/e2e/users.rs`, `tests/e2e/team_groups.rs`, `tests/e2e/saml.rs`, `tests/e2e/ip_access.rs`, `tests/e2e/data_archive.rs`
- **What:**
  1. Review each E2E test file. If any test invokes a write operation (create, update, delete, set, enable, disable, set-status, set-idp, set-active), add `"--yes"` to the argument list. Read-only tests (list, get, search) should NOT need changes.
  2. Most existing E2E tests are read-only (list, get, search), so changes should be minimal. Check each file.
  3. Run `cargo fmt && cargo clippy && cargo test` to verify everything passes.
  4. Verify unit tests in `tests/api_keys/`, `tests/roles/`, `tests/scopes/`, `tests/users/`, `tests/team_groups/`, `tests/saml/`, `tests/ip_access/`, `tests/data_archive/` still pass - they call handler functions directly (not through main.rs) so the confirmation prompt should NOT affect them.
  5. Run `cargo install --path .` to install final binary.
- **Acceptance:** `cargo fmt --check` exits 0. `cargo clippy` exits 0. `cargo test` exits 0 (all non-ignored tests pass). `cargo build --tests` compiles. If E2E env is available: `cargo test --test e2e -- --ignored --test-threads=1` also passes.
- **Dependencies:** 1.2, 2.1, 2.2
