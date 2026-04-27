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

#[path = "e2e/alerts/mod.rs"]
mod alerts;
#[path = "e2e/dashboards/mod.rs"]
mod dashboards;
#[path = "e2e/dataprime/mod.rs"]
mod dataprime;
#[path = "e2e/logs/mod.rs"]
mod logs;
#[path = "e2e/metrics/mod.rs"]
mod metrics;
#[path = "e2e/output_formats.rs"]
mod output_formats;
#[path = "e2e/profiles_and_local.rs"]
mod profiles_and_local;
#[path = "e2e/search_fields/mod.rs"]
mod search_fields;
#[path = "e2e/spans/mod.rs"]
mod spans;
