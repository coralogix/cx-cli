//! E2E test suite for the cx CLI.
//!
//! All tests are marked `#[ignore]` so they are excluded from the default
//! `cargo test` run. To execute against a real Coralogix test team:
//!
//! ```bash
//! export CX_API_KEY=...           # or populate .env
//! export CX_REGION=stg1
//! cargo test --test e2e -- --ignored --test-threads=1
//! ```
//!
//! See `docs/development.md` for full setup instructions.

#[path = "e2e/harness.rs"]
mod harness;

#[path = "e2e/actions.rs"]
mod actions;
#[path = "e2e/ai_center/mod.rs"]
mod ai_center;
#[path = "e2e/alerts/mod.rs"]
mod alerts;
#[path = "e2e/api_keys.rs"]
mod api_keys;
#[path = "e2e/cases/mod.rs"]
mod cases;
#[path = "e2e/connectors.rs"]
mod connectors;
#[path = "e2e/contextual_data.rs"]
mod contextual_data;
#[path = "e2e/custom_enrichments.rs"]
mod custom_enrichments;
#[path = "e2e/dashboards/mod.rs"]
mod dashboards;
#[path = "e2e/data_archive.rs"]
mod data_archive;
#[path = "e2e/data_usage.rs"]
mod data_usage;
#[path = "e2e/dataprime/mod.rs"]
mod dataprime;
#[path = "e2e/e2m.rs"]
mod e2m;
#[path = "e2e/enrichments.rs"]
mod enrichments;
#[path = "e2e/extensions.rs"]
mod extensions;
#[path = "e2e/infra.rs"]
mod infra;
#[path = "e2e/integrations.rs"]
mod integrations;
#[path = "e2e/ip_access.rs"]
mod ip_access;
#[path = "e2e/logs/mod.rs"]
mod logs;
#[path = "e2e/metrics/mod.rs"]
mod metrics;
#[path = "e2e/olly/mod.rs"]
mod olly;
#[path = "e2e/output_formats.rs"]
mod output_formats;
#[path = "e2e/parsing_rules.rs"]
mod parsing_rules;
#[path = "e2e/profiles_and_local.rs"]
mod profiles_and_local;
#[path = "e2e/read_only.rs"]
mod read_only;
#[path = "e2e/recording_rules.rs"]
mod recording_rules;
#[path = "e2e/retentions.rs"]
mod retentions;
#[path = "e2e/roles.rs"]
mod roles;
#[path = "e2e/routers.rs"]
mod routers;
#[path = "e2e/scopes.rs"]
mod scopes;
#[path = "e2e/search_fields/mod.rs"]
mod search_fields;
#[path = "e2e/service_catalog.rs"]
mod service_catalog;
#[path = "e2e/slos.rs"]
mod slos;
#[path = "e2e/spans/mod.rs"]
mod spans;
#[path = "e2e/suppression_rules.rs"]
mod suppression_rules;
#[path = "e2e/tco_policies.rs"]
mod tco_policies;
#[path = "e2e/team_groups.rs"]
mod team_groups;
#[path = "e2e/users.rs"]
mod users;
#[path = "e2e/views.rs"]
mod views;
#[path = "e2e/webhooks.rs"]
mod webhooks;
#[path = "e2e/whoami.rs"]
mod whoami;
