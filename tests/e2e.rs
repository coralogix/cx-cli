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

#[path = "e2e/alerts.rs"]
mod alerts;
#[path = "e2e/dashboards.rs"]
mod dashboards;
#[path = "e2e/e2m.rs"]
mod e2m;
#[path = "e2e/dataprime_query.rs"]
mod dataprime_query;
#[path = "e2e/logs.rs"]
mod logs;
#[path = "e2e/metrics.rs"]
mod metrics;
#[path = "e2e/profiles_and_local.rs"]
mod profiles_and_local;
#[path = "e2e/recording_rules.rs"]
mod recording_rules;
#[path = "e2e/search_fields.rs"]
mod search_fields;
#[path = "e2e/slos.rs"]
mod slos;
#[path = "e2e/spans.rs"]
mod spans;
