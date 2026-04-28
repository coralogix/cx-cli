# Plan: CLI Command Hierarchy Reorganization (Option D)

| Field | Value |
|-------|-------|
| Status | in-progress |
| Created | 2026-04-28 |
| Ticket | N/A |
| Branch | liranhason/cli-hierarchy |

## Context

The `cx` CLI currently has **37 flat top-level commands**, making `cx --help` overwhelming. We're implementing **Option D (Expert Flat Domains)** from `plans/cli_command_grouping_options.md`: keep all commands at the top level but reduce from 37 → 26 via smart merges and renames, organize `--help` output with display-only headings, and add a `cx schema` command for structured agent discovery.

No command modules (`src/commands/*.rs`) or API modules (`src/api/*.rs`) change — only the CLI layer (`src/main.rs`), tests, skills, and docs.

## Architecture Decisions

- **Decision 1: Help headings via custom help_template + after_help.** *(Updated after 1.1 spike)* Clap 4.5's `next_help_heading` and `subcommand_help_heading` do NOT support multiple heading groups for subcommands — `next_help_heading` only affects arguments/options, and `subcommand_help_heading` renames the single "Commands:" heading for all subcommands. Instead, we use a custom `help_template` on the `Cli` struct that replaces `{subcommands}` with `{after-help}`, and define the organized command listing (with domain headings) as the `after_help` text. The `Commands` enum stays flat — no flatten groups, no sub-enums for grouping. Individual commands remain fully functional and discoverable via `cx help <cmd>`. **Consequence for M1 tasks:** Task 1.2 (split Commands into group sub-enums) is no longer needed — the flat enum stays. Task 1.3 (renames) and all M2 tasks (merges) proceed unchanged since they modify the flat enum and dispatch match arms directly.
- **Decision 2:** Merges create wrapper enums in `main.rs` that delegate to existing `*Cmd` enums. E.g., `AlertsCmd` gains a `Schedulers` variant wrapping `AlertSchedulersCmd`. No changes to `src/commands/` handler functions.
- **Decision 3:** Renames use Clap's `#[command(name = "...")]` attribute. The Rust enum variant name stays the same; only the CLI-facing name changes.
- **Decision 4:** `cx schema` outputs the command tree as JSON, built by walking the Clap `Command` structure at runtime. No hand-maintained schema file.
- **Decision 5:** No backward-compatibility aliases — pre-1.0 CLI, clean break.

## New Command Structure (Option D)

After this change, `cx --help` will show:

```
Coralogix CLI — query observability data from the terminal.

Usage: cx [OPTIONS] <COMMAND>

Query:
  logs               Query logs using DataPrime syntax
  spans              Query spans using DataPrime syntax
  metrics            Query metrics using PromQL
  dataprime          DataPrime language reference and raw queries
  search-fields      Search log/span fields semantically

Observe:
  dashboards         Manage dashboards and dashboard folders
  views              Manage saved views and view folders
  slos               Manage SLO definitions

Detect & Respond:
  alerts             Manage alert definitions and schedulers
  incidents          Manage and triage incidents

Notifications:
  notifications      Manage connectors, routers, presets, and notification testing
  webhooks           Manage outgoing webhooks and automation actions

Data Pipeline:
  rules              Manage log parsing rule groups
  enrichments        Manage enrichment rules and custom enrichment tables
  e2m                Manage Events2Metrics definitions
  recording-rules    Manage Prometheus recording rule groups

Cost & Storage:
  usage              View data usage and consumption metrics
  tco                Manage TCO policies and settings
  retentions         Manage data retention settings
  quotas             Manage quota rules
  archive            Manage data archive storage configuration

Integrations:
  integrations       Manage integrations, extensions, and contextual data

Access:
  iam                Manage API keys, roles, scopes, users, groups, SAML, and IP access

Agent:
  schema             Output the full command tree as JSON for agent consumption

Local:
  profiles           Manage profiles (list, add, delete, set-default)
  cleanup            Remove stale temp files
```

### Full merge/rename mapping

| Old command | New command | Change type |
|---|---|---|
| `logs` | `logs` | unchanged |
| `spans` | `spans` | unchanged |
| `metrics` | `metrics` | unchanged |
| `dataprime` | `dataprime` | unchanged |
| `search-fields` | `search-fields` | unchanged |
| `dashboards` | `dashboards` | unchanged |
| `views` | `views` | unchanged |
| `slos` | `slos` | unchanged |
| `incidents` | `incidents` | unchanged |
| `e2m` | `e2m` | unchanged |
| `recording-rules` | `recording-rules` | unchanged |
| `retentions` | `retentions` | unchanged |
| `profiles` | `profiles` | unchanged |
| `cleanup` | `cleanup` | unchanged |
| `alerts` | `alerts` | **merge**: absorbs `alert-schedulers` as `cx alerts schedulers ...` |
| `alert-schedulers` | _(removed)_ | merged into `alerts` |
| `connectors` | _(removed)_ | merged into `notifications` as `cx notifications connectors ...` |
| `routers` | _(removed)_ | merged into `notifications` as `cx notifications routers ...` |
| `presets` | _(removed)_ | merged into `notifications` as `cx notifications presets ...` |
| `notification-test` | _(removed)_ | merged into `notifications` as `cx notifications test ...` |
| `webhooks` | `webhooks` | **merge**: absorbs `actions` as `cx webhooks actions ...` |
| `actions` | _(removed)_ | merged into `webhooks` |
| `rule-groups` | `rules` | **rename** |
| `enrichments` | `enrichments` | **merge**: absorbs `custom-enrichments` as `cx enrichments custom ...` |
| `custom-enrichments` | _(removed)_ | merged into `enrichments` |
| `data-usage` | `usage` | **rename** |
| `tco-policies` | `tco` | **rename** |
| `quota-rules` | `quotas` | **rename** |
| `data-archive` | `archive` | **rename** |
| `integrations` | `integrations` | **merge**: absorbs `extensions` + `contextual-data` |
| `extensions` | _(removed)_ | merged into `integrations` as `cx integrations extensions ...` |
| `contextual-data` | _(removed)_ | merged into `integrations` as `cx integrations contextual-data ...` |
| `api-keys` | _(removed)_ | merged into `iam` as `cx iam api-keys ...` |
| `roles` | _(removed)_ | merged into `iam` as `cx iam roles ...` |
| `scopes` | _(removed)_ | merged into `iam` as `cx iam scopes ...` |
| `users` | _(removed)_ | merged into `iam` as `cx iam users ...` |
| `team-groups` | _(removed)_ | merged into `iam` as `cx iam groups ...` |
| `saml` | _(removed)_ | merged into `iam` as `cx iam saml ...` |
| `ip-access` | _(removed)_ | merged into `iam` as `cx iam ip-access ...` |
| _(new)_ | `schema` | **new**: outputs command tree as JSON |
| _(new)_ | `notifications` | **new**: wraps connectors + routers + presets + test |

## Milestones Overview

1. **Scannable help output** (3 tasks) — A user running `cx --help` sees commands organized by domain with visual headings and shorter names, instead of an alphabetical wall of 37 entries.
2. **Unified command domains** (7 tasks) — Related commands are grouped under single top-level entries. A user managing notifications types `cx notifications` instead of guessing between 6 separate top-level commands. Top-level count drops from 37 → 26.
3. **Agent-native discovery** (2 tasks) — An AI agent runs `cx schema` and gets a structured JSON command tree in one call — no help-text parsing, no multi-hop `--help` exploration.
4. **Green test suite + consistent docs** (8 tasks) — All tests pass against the new structure, and all skills/docs/CLAUDE.md reference correct command paths. E2E test updates, skill updates, and doc updates can run in parallel since they're independent.

---

## Milestone 1: Scannable Help Output

**Why this matters:** Today a user running `cx --help` sees 37 commands in a flat alphabetical list. They can't visually separate query commands from IAM commands from cost commands. After this milestone, `cx --help` is organized into labeled sections (Query, Observe, Detect & Respond, etc.) and 5 verbose command names are shortened. A new user can scan the help output and immediately find the domain they care about. An agent parsing `--help` can use the headings to narrow its search space.

**Success criteria:** Running `cx --help` shows commands grouped under labeled headings. Running `cx rules list`, `cx tco list`, `cx quotas get`, `cx usage summary`, `cx archive metrics get` all produce correct output. The old verbose names (`cx rule-groups`, `cx tco-policies`, etc.) no longer resolve. `cargo build` succeeds.

**Key decisions:** Renames use `#[command(name = "rules")]` on the enum variant — the Rust variant name stays `RuleGroups` to avoid touching handler code. Headings require the flatten-group pattern (see Architecture Decision 1) — `next_help_heading` only works on `#[command(flatten)]` variants in Clap 4.x.

### 1.1 [x] Spike: verify Clap heading pattern works *(completed 2026-04-28)*
- **Files:** `src/main.rs`
- **What:** Before restructuring all 37 variants, do a minimal proof-of-concept. Take 2-3 existing variants (e.g., `Logs`, `Spans`) and extract them into a small `QueryGroup` sub-enum. Flatten it back into `Commands` with `#[command(flatten, next_help_heading = "Query")]`. Verify that `cx --help` shows a "Query:" heading above `logs` and `spans`, and that `cx logs '...'` still works identically. Also verify the dispatch still compiles — the match arm changes from `Commands::Logs { ... }` to `Commands::Query(QueryGroup::Logs { ... })`. If the flatten pattern doesn't produce headings in help, investigate alternatives (builder API, `help_heading` on individual variants) before committing to the approach. **This task is a gate: do NOT proceed to 1.2 until headings render correctly.**
- **Acceptance:** `cx --help` shows a "Query:" heading above logs/spans. `cx logs 'filter $m.severity == ERROR'` works. `cargo build` succeeds.
- **Dependencies:** None

### 1.2 [x] Split Commands enum into heading groups *(completed 2026-04-28 — superseded by help_template approach)*
- **Files:** `src/main.rs`
- **What:** *(Superseded)* The 1.1 spike proved that Clap's flatten/`next_help_heading` doesn't work for subcommand grouping. Instead, organized headings were implemented via `help_template` + `after_help` directly in task 1.1. The `Commands` enum stays flat. This task is complete — no further work needed.
- **Acceptance:** `cx --help` shows all commands under labeled headings in the correct order. Every existing command still works. `cargo build` succeeds.
- **Dependencies:** 1.1

### 1.3 [x] Rename 5 commands to shorter names *(completed 2026-04-28)*
- **Files:** `src/main.rs`
- **What:** Add `#[command(name = "rules")]` to `RuleGroups` variant, `#[command(name = "tco")]` to `TcoPolicies`, `#[command(name = "quotas")]` to `QuotaRules`, `#[command(name = "usage")]` to `DataUsage`, `#[command(name = "archive")]` to `DataArchive`. Update the `after_help` examples in each to use the new names. The dispatch `match` arms don't change since they match on Rust variant names.
- **Acceptance:** `cx rules list`, `cx tco list`, `cx quotas get`, `cx usage summary`, `cx archive metrics get` all work. Old names (`cx rule-groups`, etc.) no longer work. `cargo build` succeeds.
- **Dependencies:** 1.2

---

## Milestone 2: Unified Command Domains

**Why this matters:** Today a user who wants to configure notification delivery has to know that `connectors`, `routers`, `presets`, and `notification-test` are four separate top-level commands. After this milestone, they type `cx notifications` and see all four in one place. The same applies to IAM (7 commands → `cx iam`), alert management (alerts + schedulers → `cx alerts`), and three other domains. The top-level count drops from 37 → 26, and related commands are discoverable together — which is the whole point of this project.

**Success criteria:** Every merged command works through its new path (e.g., `cx iam api-keys list`, `cx notifications connectors list`). `cx --help` shows exactly 26 commands (plus help). All handler functions in `src/commands/*.rs` are called unchanged — zero logic changes. `cargo build` and `cargo clippy` pass.

**Key decisions:** Wrapper enums are defined in `main.rs` alongside existing enums. Handler dispatch is rewired in the `match cli.command` block. No changes to `src/commands/` or `src/api/`.

**Important: each M2 task modifies `main.rs` heavily.** Tasks 2.1–2.6 are independent of each other but all edit the same file. The implementing agent must **read the current state of `main.rs` at the start of each task** — do not rely on line numbers or code snippets from the plan. After each merge, variants move between group enums (from the flatten pattern in M1), so the dispatch match structure will look different than when the plan was written.

### 2.1 [ ] Merge `alert-schedulers` into `alerts`
- **Files:** `src/main.rs`
- **What:** Add a `Schedulers` variant to `AlertsCmd` (or create a new wrapper `NewAlertsCmd` enum with the existing alert subcommands + `Schedulers { cmd: AlertSchedulersCmd }`). Remove the `AlertSchedulers` variant from `Commands`. Rewire the dispatch: `Commands::Alerts { cmd } => match cmd { ... NewAlertsCmd::Schedulers { cmd } => match cmd { AlertSchedulersCmd::* => commands::alert_schedulers::run_*(...) } }`. Update `after_help` examples.
- **Acceptance:** `cx alerts list` works. `cx alerts schedulers list` works. `cx alert-schedulers` no longer exists. `cargo build` succeeds.
- **Dependencies:** 1.3

### 2.2 [ ] Merge `actions` into `webhooks`
- **Files:** `src/main.rs`
- **What:** Add an `Actions` variant wrapping `ActionsCmd` to `WebhooksCmd` (or create a wrapper). Remove the `Actions` variant from `Commands`. Rewire dispatch. Update `after_help`.
- **Acceptance:** `cx webhooks list` works. `cx webhooks actions list` works. `cx actions` no longer exists. `cargo build` succeeds.
- **Dependencies:** 1.3

### 2.3 [ ] Merge `custom-enrichments` into `enrichments`
- **Files:** `src/main.rs`
- **What:** Add a `Custom` variant wrapping `CustomEnrichmentsCmd` to `EnrichmentsCmd` (or create a wrapper). Remove the `CustomEnrichments` variant from `Commands`. Rewire dispatch. Update `after_help`.
- **Acceptance:** `cx enrichments list` works. `cx enrichments custom list` works. `cx custom-enrichments` no longer exists. `cargo build` succeeds.
- **Dependencies:** 1.3

### 2.4 [ ] Create `notifications` command (merge connectors + routers + presets + notification-test)
- **Files:** `src/main.rs`
- **What:** Create a new `NotificationsCmd` enum with variants: `Connectors { cmd: ConnectorsCmd }`, `Routers { cmd: RoutersCmd }`, `Presets { cmd: PresetsCmd }`, `Test { cmd: NotificationTestCmd }`. Add a `Notifications` variant to `Commands`. Remove the individual `Connectors`, `Routers`, `Presets`, `NotificationTest` variants from `Commands`. Rewire dispatch. Add `after_help` examples showing `cx notifications connectors list`, etc.
- **Acceptance:** `cx notifications connectors list`, `cx notifications routers list`, `cx notifications presets list`, `cx notifications test ...` all work. Old flat commands no longer exist. `cargo build` succeeds.
- **Dependencies:** 1.3

### 2.5 [ ] Merge `extensions` + `contextual-data` into `integrations`
- **Files:** `src/main.rs`
- **What:** The existing `IntegrationsCmd` already has CRUD subcommands (`List`, `Get`, `Create`, `Delete`). Create a new `IntegrationsCmdExpanded` enum that **copies all existing variants from `IntegrationsCmd`** (List, Get, Create, Delete) and adds `Extensions { cmd: ExtensionsCmd }` and `ContextualData { cmd: ContextualDataCmd }`. Replace `IntegrationsCmd` with this new enum in the `Commands` variant (or rename it). Do NOT nest the old enum — the CRUD subcommands must stay at the same level as `extensions` and `contextual-data` so the user sees `cx integrations list` alongside `cx integrations extensions list`. Remove the `Extensions` and `ContextualData` variants from `Commands`. Rewire dispatch. Update `after_help`.
- **Acceptance:** `cx integrations list` still works. `cx integrations extensions list` works. `cx integrations contextual-data list` works. `cargo build` succeeds.
- **Dependencies:** 1.3

### 2.6 [ ] Create `iam` command (merge api-keys + roles + scopes + users + team-groups + saml + ip-access)
- **Files:** `src/main.rs`
- **What:** Create a new `IamCmd` enum with variants: `ApiKeys { cmd: ApiKeysCmd }`, `Roles { cmd: RolesCmd }`, `Scopes { cmd: ScopesCmd }`, `Users { cmd: UsersCmd }`, `Groups { cmd: TeamGroupsCmd }` (note: renamed from `team-groups` to `groups` via `#[command(name = "groups")]`), `Saml { cmd: SamlCmd }`, `IpAccess { cmd: IpAccessCmd }`. Add `Iam` variant to `Commands`. Remove `ApiKeys`, `Roles`, `Scopes`, `Users`, `TeamGroups`, `Saml`, `IpAccess` from `Commands`. Rewire dispatch. Add comprehensive `after_help`.
- **Acceptance:** `cx iam api-keys list`, `cx iam roles list`, `cx iam scopes list`, `cx iam users search`, `cx iam groups list`, `cx iam saml get`, `cx iam ip-access get` all work. `cargo build` succeeds.
- **Dependencies:** 1.3

### 2.7 [ ] Final cleanup: consolidate groups and verify help output
- **Files:** `src/main.rs`
- **What:** After all merges, the flatten-group sub-enums from M1 need updating. Some groups will have lost variants (e.g., the temporary "Notifications" heading group had `Connectors`, `Routers`, `Presets`, `NotificationTest` — now they're gone, replaced by a single `Notifications` wrapper). Others gained new wrapper variants (`notifications`, `iam`). Consolidate the group enums to match the final target: `QueryGroup` (logs, spans, metrics, dataprime, search-fields), `ObserveGroup` (dashboards, views, slos), `DetectGroup` (alerts, incidents), `NotificationsGroup` (notifications, webhooks), `PipelineGroup` (rules, enrichments, e2m, recording-rules), `CostGroup` (usage, tco, retentions, quotas, archive), `IntegrationsGroup` (integrations), `AccessGroup` (iam), `LocalGroup` (profiles, cleanup). Remove any empty or temporary groups. Ensure the variant order within each group matches the target `--help` output. Run `cargo fmt`, `cargo clippy`, fix any warnings.
- **Acceptance:** `cx --help` matches the target output in this plan exactly. `cargo clippy` and `cargo fmt` clean. `cargo build` succeeds.
- **Dependencies:** 2.1–2.6

---

## Milestone 3: Agent-Native Discovery (`cx schema`)

**Why this matters:** Today an AI agent using cx must run `cx --help`, parse human-readable text, then run `cx <cmd> --help` for each command it needs — multiple round-trips of unstructured text parsing. After this milestone, an agent runs `cx schema` once and gets a complete, structured JSON description of every command, subcommand, argument, type, and default value. This turns cx into a first-class tool for AI-assisted observability workflows — the agent knows exactly what it can do and how to invoke it, in a single call.

**Success criteria:** `cx schema` outputs valid JSON. `cx schema | jq .` parses successfully. The JSON includes all 26 commands with their full subcommand trees and argument definitions (name, type, required/optional, default, description). The command runs without API credentials (local-only).

**Key decisions:** Build the schema by walking Clap's `Command` structure at runtime (using `Cli::command()` to get the top-level `Command`, then recursing through subcommands). No hand-maintained schema file — the JSON is always in sync with the actual CLI. Output format is a JSON object with a `commands` array.

### 3.1 [ ] Implement `cx schema` command
- **Files:** `src/main.rs`, `src/commands/schema.rs`, `src/commands/mod.rs`
- **What:** Create `src/commands/schema.rs` with a function that takes a `clap::Command` and recursively builds a JSON tree. Add a `Schema` variant to `Commands` in `main.rs` (in `LocalGroup`, or a new `AgentGroup` with heading "Agent"). The command should NOT require API credentials (it's local-only). Handle it before `build_targets()` like `Profiles` and `Cleanup` are handled. Add to `src/commands/mod.rs`. The JSON output format should look like:
  ```json
  {
    "name": "cx",
    "version": "0.1.0",
    "commands": [
      {
        "name": "slos",
        "description": "Manage SLO definitions",
        "subcommands": [
          {
            "name": "list",
            "description": "List all SLOs",
            "arguments": []
          },
          {
            "name": "get",
            "description": "Get a single SLO by ID",
            "arguments": [
              { "name": "slo_id", "type": "string", "required": true, "description": "SLO definition ID" }
            ]
          }
        ]
      },
      {
        "name": "iam",
        "description": "Manage API keys, roles, scopes, users, groups, SAML, and IP access",
        "subcommands": [
          {
            "name": "api-keys",
            "description": "Manage API keys",
            "subcommands": [
              { "name": "list", "description": "List all API keys", "arguments": [] }
            ]
          }
        ]
      }
    ]
  }
  ```
  Each node includes `name`, `description`, and optionally `subcommands` (recursive) and `arguments` (with name, type, required, default, description). Build this by walking Clap's `Command::get_subcommands()` and `Command::get_arguments()` recursively.
- **Acceptance:** `cx schema` outputs valid JSON. `cx schema | jq .` parses successfully. The output includes all 26 commands with their subcommands and arguments. `cargo test` passes.
- **Dependencies:** 2.7

### 3.2 [ ] Add unit test for schema output
- **Files:** `tests/schema.rs`
- **What:** Add a test that calls the schema generation function and verifies: (1) output is valid JSON, (2) top-level has the expected number of commands (26 including schema itself), (3) key commands have expected subcommands (e.g., `alerts` has `list`, `get`, `create`, `enable`, `disable`, `events`, `event-stats`, `schedulers`; `iam` has `api-keys`, `roles`, `scopes`, `users`, `groups`, `saml`, `ip-access`).
- **Acceptance:** `cargo test schema` passes.
- **Dependencies:** 3.1

---

## Milestone 4: Green Test Suite + Consistent Documentation

**Why this matters:** The restructuring changed every command path that moved under a wrapper or got renamed. Tests using old paths won't compile. Skills and docs referencing old paths will mislead agents and contributors. After this milestone: `cargo test` passes cleanly (proving zero functional regressions), all skill files reference correct command paths (agents invoke the right commands), and CLAUDE.md/docs accurately describe the new hierarchy (contributors extend the CLI correctly).

**Success criteria:** `cargo test` passes with zero failures. `cargo test --test e2e -- --ignored --test-threads=1` compiles (runtime pass requires `CX_API_KEY`). `cargo clippy` clean. `grep` for old command paths across skills/, .claude/skills/, docs/, and CLAUDE.md returns zero matches. `cargo install --path .` succeeds.

**Key decisions:** Unit/integration tests call `commands::*::run_*()` directly and should still compile unchanged since handler functions weren't modified. Only E2E tests (string-based CLI invocations) need path updates. Test updates and doc updates are independent and can be executed in parallel.

### 4.1 [ ] Update E2E tests for merged commands: alerts, webhooks, enrichments
- **Files:** `tests/e2e/alert_schedulers.rs`, `tests/e2e/actions.rs`, `tests/e2e/custom_enrichments.rs`
- **What:** Update command path arrays. `&["alert-schedulers", ...]` → `&["alerts", "schedulers", ...]`. `&["actions", ...]` → `&["webhooks", "actions", ...]`. `&["custom-enrichments", ...]` → `&["enrichments", "custom", ...]`.
- **Acceptance:** E2E tests compile.
- **Dependencies:** 2.7

### 4.2 [ ] Update E2E tests for `notifications` (connectors, routers, presets, notification-test)
- **Files:** `tests/e2e/connectors.rs`, `tests/e2e/routers.rs`, `tests/e2e/presets.rs`, `tests/e2e/notification_testing.rs`
- **What:** Prepend `"notifications"` to all command arrays. `&["connectors", ...]` → `&["notifications", "connectors", ...]`. etc.
- **Acceptance:** E2E tests compile.
- **Dependencies:** 2.7

### 4.3 [ ] Update E2E tests for `integrations` (extensions, contextual-data)
- **Files:** `tests/e2e/extensions.rs`, `tests/e2e/contextual_data.rs`
- **What:** Prepend `"integrations"` to command arrays. `&["extensions", ...]` → `&["integrations", "extensions", ...]`. `&["contextual-data", ...]` → `&["integrations", "contextual-data", ...]`.
- **Acceptance:** E2E tests compile.
- **Dependencies:** 2.7

### 4.4 [ ] Update E2E tests for `iam` (api-keys, roles, scopes, users, team-groups, saml, ip-access)
- **Files:** `tests/e2e/api_keys.rs`, `tests/e2e/roles.rs`, `tests/e2e/scopes.rs`, `tests/e2e/users.rs`, `tests/e2e/team_groups.rs`, `tests/e2e/saml.rs`, `tests/e2e/ip_access.rs`
- **What:** Prepend `"iam"` to command arrays. `&["api-keys", ...]` → `&["iam", "api-keys", ...]`. `&["team-groups", ...]` → `&["iam", "groups", ...]` (note: also renamed to `groups`).
- **Acceptance:** E2E tests compile.
- **Dependencies:** 2.7

### 4.5 [ ] Update E2E tests for renamed commands
- **Files:** `tests/e2e/rule_groups.rs`, `tests/e2e/tco_policies.rs`, `tests/e2e/quota_rules.rs`, `tests/e2e/data_usage.rs`, `tests/e2e/data_archive.rs`
- **What:** Update command names in arrays. `&["rule-groups", ...]` → `&["rules", ...]`. `&["tco-policies", ...]` → `&["tco", ...]`. `&["quota-rules", ...]` → `&["quotas", ...]`. `&["data-usage", ...]` → `&["usage", ...]`. `&["data-archive", ...]` → `&["archive", ...]`.
- **Acceptance:** E2E tests compile.
- **Dependencies:** 2.7

### 4.6 [ ] Update skills
- **Files:** `skills/cx-alerts/SKILL.md`, `skills/*/SKILL.md`, `.claude/skills/add-command/SKILL.md`, `.claude/skills/add-skill/SKILL.md`
- **What:** Update all references to old command paths. Key changes: `cx alert-schedulers` → `cx alerts schedulers` in the cx-alerts skill. Search all other skill files for references to removed/renamed commands. The `add-command` skill should be updated to explain how to add a command under an existing wrapper (e.g., adding to `iam`) vs. at the top level, and how to add to a flatten-group for help-text headings.
- **Acceptance:** `grep -r "cx alert-schedulers\|cx connectors\|cx routers\|cx presets\|cx notification-test\|cx actions\|cx custom-enrichments\|cx rule-groups\|cx tco-policies\|cx quota-rules\|cx data-usage\|cx data-archive\|cx extensions\|cx contextual-data\|cx api-keys\b\|cx roles\b\|cx scopes\b\|cx users\b\|cx team-groups\|cx saml\b\|cx ip-access" skills/ .claude/skills/` returns no matches.
- **Acceptance:** Skill files have no stale command paths.
- **Dependencies:** 2.7

### 4.7 [ ] Update CLAUDE.md and docs/
- **Files:** `CLAUDE.md`, `docs/architecture.md`, `docs/adding-a-command.md`, `docs/adding-a-skill.md`, `docs/development.md`, `docs/configuration.md`, `docs/agents-output.md`
- **What:** Add a "Command Hierarchy" section to CLAUDE.md showing the new structure (headings + commands). Update the architecture section to explain the flatten-group and wrapper enum patterns. Update the "Skill Coverage" table. Update example commands throughout. Mention `cx schema` as an agent discovery tool. In docs/: search all files for old command paths and update. Update `adding-a-command.md` to explain how to add a command to a wrapper (e.g., `iam`) vs. top-level vs. flatten-group. Add `cx schema` to `agents-output.md`.
- **Acceptance:** `grep` for old flat paths returns no hits in CLAUDE.md or docs/. CLAUDE.md accurately describes the CLI structure.
- **Dependencies:** 3.1

### 4.8 [ ] Verify all tests pass and final install
- **Files:** N/A
- **What:** Run `cargo fmt`, `cargo clippy`, `cargo test` (this includes the schema test from 3.2). Verify zero failures. Run `cargo install --path .`. Run `cx --help`, `cx alerts --help`, `cx iam --help`, `cx notifications --help`, `cx schema | jq .` to verify everything works. Visually confirm the `--help` output matches the target.
- **Acceptance:** All checks pass. `cx --help` output matches the plan. `cx schema` outputs valid JSON with all 26 commands.
- **Dependencies:** 4.1–4.7, 3.2
