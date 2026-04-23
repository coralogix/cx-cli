# Contributing to cx

Thank you for your interest in contributing to `cx`, the Coralogix CLI. This guide covers who can contribute, how code is owned and reviewed, and the process for adding new functionality.

## Who Can Contribute

**Coralogix teams** — Each team is encouraged to add commands and skills for the observability domains they own. This is the primary contribution path.

**External contributors** — Contributions from outside Coralogix are welcome. The same process applies: open an issue first, then follow the lifecycle below.

## Ownership Model

Every command domain has a designated owning team. The owning team is responsible for the correctness, quality, and evolution of their domain's code and skills.

| Domain | Scope | Owner |
|--------|-------|-------|
| Core infrastructure | `src/execution.rs`, `src/config.rs`, `src/spill.rs`, `src/main.rs`, `src/error.rs`, `src/api/client.rs` | `@coralogix/team-cxai` |
| Logs | `src/commands/logs.rs`, `skills/query-logs/` | Domain team |
| Metrics | `src/commands/metrics.rs`, `src/commands/metrics/`, `skills/metrics-query/` | Domain team |
| Traces | `src/commands/spans.rs`, `skills/query-spans/` | Domain team |
| Alerts | `src/commands/alerts.rs`, `src/api/alerts.rs`, `skills/cx-alerts/` | Domain team |
| Dashboards | `src/commands/dashboards.rs`, `src/api/dashboards.rs` | Domain team |
| Search fields | `src/commands/search_fields.rs`, `src/api/semantic_search.rs` | Domain team |
| Documentation | `docs/*` | `@coralogix/team-cxai` |

> **Note:** `@coralogix/team-cxai` is the core team and has final review authority on all changes. Domain ownership will be formally encoded in `CODEOWNERS` as teams onboard.

## PR Review Process

| What you're changing | Required reviewers |
|----------------------|--------------------|
| Command files (`src/commands/*`, `src/api/*`, `skills/*`) | Domain owning team **+** `@coralogix/team-cxai` |
| Shared infrastructure (`src/execution.rs`, `src/config.rs`, `src/spill.rs`, `src/main.rs`, `src/error.rs`, `src/api/client.rs`) | `@coralogix/team-cxai` only |
| Documentation (`docs/*`, `CONTRIBUTING.md`, `README.md`) | `@coralogix/team-cxai` |
| CI / GitHub workflows (`.github/*`) | `@coralogix/team-cxai` |

All PRs require at least one approving review before merge. PRs that touch shared infrastructure require approval from the core team regardless of other approvals.

## Command Lifecycle

Adding a new command follows this lifecycle:

1. **Proposal** — Open an issue describing the command, its target API, and the intended user workflow.
2. **Design review** — The core team reviews the proposal and confirms the archetype (DataPrime-based or REST-based). See [architecture.md](docs/architecture.md) for the two archetypes.
3. **Implementation** — Follow the step-by-step guide in [adding-a-command.md](docs/adding-a-command.md). Every new command **must** ship with a corresponding user-facing skill — see [adding-a-skill.md](docs/adding-a-skill.md).
4. **PR review** — Open a PR. Dual review applies (domain team + core team).
5. **Merge** — Squash-merge into `master` after approval.
6. **Release** — The release workflow builds and publishes binaries automatically.

## Security Rules

- **API keys must never be logged or printed to stdout.** Credentials are resolved from environment variables (`CX_API_KEY`) or the config file (`~/.cx/config.toml`). Never write code that exposes secrets in output, logs, or error messages.
- **Validate inputs at system boundaries.** User-provided query strings, IDs, and timestamps should be validated before use.
- **Report vulnerabilities** following the process in [SECURITY.md](SECURITY.md).

## Skill Requirement

**Every new command must ship with a corresponding user-facing skill in `skills/`.** Skills teach AI agents how to use your command effectively — they are a required part of every command PR, not optional.

See [Adding a Skill](docs/adding-a-skill.md) for directory structure, frontmatter conventions, and a copy-pasteable template.

## Getting Started

1. Read the [architecture overview](docs/architecture.md) to understand the execution pipeline
2. Read the [development guide](docs/development.md) for build, test, and lint commands
3. Follow [adding a command](docs/adding-a-command.md) for the step-by-step implementation guide
4. Follow [adding a skill](docs/adding-a-skill.md) to create the required user-facing skill
