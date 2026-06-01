# Contributing to cx

Thank you for your interest in contributing to `cx`, the Coralogix CLI. This guide covers who can contribute, how code is owned and reviewed, and the process for adding new functionality.

## Who Can Contribute

**Coralogix teams** - Each team is encouraged to add commands and skills for the observability domains they own. This is the primary contribution path.

**External contributors** - Contributions from outside Coralogix are welcome. The same process applies: open an issue first, then follow the lifecycle below.

## Ownership Model

Every command domain has a designated owning team. The owning team is responsible for the correctness, quality, and evolution of their domain's code and skills.

| Domain | Scope | Owner |
|--------|-------|-------|
| Core infrastructure | `src/main.rs`, `src/execution.rs`, `src/config.rs`, `src/safety.rs`, `src/api_client.rs`, `src/spill.rs`, `src/error.rs` | `@coralogix/team-cxai` |
| Logs | `src/commands/logs/`, `skills/cx-query-logs/` | Domain team |
| Metrics | `src/commands/metrics/`, `skills/cx-metrics-query/` | Domain team |
| Traces | `src/commands/spans/`, `skills/cx-query-spans/` | Domain team |
| Alerts | `src/commands/alerts/`, `skills/cx-alerts/` | Domain team |
| Dashboards | `src/commands/dashboards/`, `tests/dashboards/`, `tests/e2e/dashboards/`, `tests/dashboard_commands.rs`, `tests/dashboard_queries.rs`, `skills/cx-dashboards/`, `skills/cx-search-dashboard/` | `@coralogix/team-dashboards` |
| Search fields | `src/commands/search_fields/`, `src/commands/dataprime/semantic_search.rs` | Domain team |
| Incidents & SLOs | `src/commands/incidents/`, `src/commands/slos/`, `skills/cx-incident-management/` | Domain team |
| Notifications | `src/commands/connectors/`, `src/commands/routers/`, `src/commands/presets/`, `src/commands/notification_testing/` | Domain team |
| Webhooks | `src/commands/webhooks/`, `src/commands/actions/` | Domain team |
| IAM | `src/commands/api_keys/`, `src/commands/roles/`, `src/commands/scopes/`, `src/commands/users/`, `src/commands/team_groups/`, `src/commands/ip_access/`, `skills/cx-platform-admin/` | Domain team |
| Data pipeline | `src/commands/parsing_rules/`, `src/commands/enrichments/`, `src/commands/custom_enrichments/`, `src/commands/e2m/`, `src/commands/recording_rules/`, `skills/cx-data-pipeline/` | Domain team |
| Cost & storage | `src/commands/data_usage/`, `src/commands/tco_policies/`, `src/commands/retentions/`, `src/commands/data_archive/`, `skills/cx-cost-optimization/` | Domain team |
| Integrations | `src/commands/integrations/`, `src/commands/extensions/`, `src/commands/contextual_data/`, `skills/cx-observability-setup/` | Domain team |
| Views | `src/commands/views/`, `skills/cx-observability-setup/` | Domain team |
| User-facing docs | `README.md`, `docs/*` | `@coralogix/team-technical-writers`, `@coralogix/team-cxai` |
| Contributor docs | `contributing/*`, `CONTRIBUTING.md` | `@coralogix/team-cxai` |

> **Note:** `@coralogix/team-cxai` is the core team and has final review authority on all changes. Domain ownership will be formally encoded in `CODEOWNERS` as teams onboard.

## PR Review Process

| What you're changing | Required reviewers |
|----------------------|--------------------|
| Command files (`src/commands/*`, `skills/*`) | Domain owning team **+** `@coralogix/team-cxai` |
| Shared infrastructure (`src/main.rs`, `src/execution.rs`, `src/config.rs`, `src/safety.rs`, `src/api_client.rs`, `src/spill.rs`, `src/error.rs`) | `@coralogix/team-cxai` only |
| User-facing docs (`README.md`, `docs/*`) | `@coralogix/team-technical-writers` + `@coralogix/team-cxai` |
| Contributor docs (`contributing/*`, `CONTRIBUTING.md`, `.github/*`) | `@coralogix/team-cxai` |

All PRs require at least one approving review before merge. PRs that touch shared infrastructure require approval from the core team regardless of other approvals.

## Command Lifecycle

Adding a new command follows this lifecycle:

1. **Proposal** - Open an issue describing the command, its target API, and the intended user workflow.
2. **Design review** - The core team reviews the proposal and confirms the archetype (DataPrime-based or REST-based). See [architecture.md](contributing/architecture.md) for the two archetypes.
3. **Implementation** - Follow the step-by-step guide in [adding-a-command.md](contributing/adding-a-command.md). Every new command **must** ship with a corresponding user-facing skill - see [adding-a-skill.md](contributing/adding-a-skill.md).
4. **PR review** - Open a PR. Dual review applies (domain team + core team).
5. **Merge** - Squash-merge into `master` after approval.
6. **Release** - The release workflow builds and publishes binaries automatically.

## Security Rules

- **API keys must never be logged or printed to stdout.** Credentials are resolved from environment variables (`CX_API_KEY`) or the config file (`~/.cx/config.toml`). Never write code that exposes secrets in output, logs, or error messages.
- **Validate inputs at system boundaries.** User-provided query strings, IDs, and timestamps should be validated before use.
- **Report vulnerabilities** following the process in [SECURITY.md](SECURITY.md).

## Skill Requirement

**Every new command must ship with a corresponding user-facing skill in `skills/`.** Skills teach AI agents how to use your command effectively - they are a required part of every command PR, not optional.

See [Adding a Skill](contributing/adding-a-skill.md) for directory structure, frontmatter conventions, and a copy-pasteable template.

## Getting Started

1. Read the [architecture overview](contributing/architecture.md) to understand the execution pipeline
2. Read the [development guide](contributing/development.md) for build, test, and lint commands
3. Follow [adding a command](contributing/adding-a-command.md) for the step-by-step implementation guide
4. Follow [adding a skill](contributing/adding-a-skill.md) to create the required user-facing skill
