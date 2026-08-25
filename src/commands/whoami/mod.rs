//! `cx whoami`: authenticated identity / health check.
//!
//! Runs the shared [`identity::verify_identity`] probe (`GET /identity/whoami`)
//! against a single profile and reports who the credentials belong to. It is an
//! "am I authenticated?" check — the same probe `cx init` runs at the end of
//! onboarding — not a fleet operation, so it does not fan out across profiles:
//! whoami is readable by every valid key regardless of scopes, so a success is
//! a definitive "authenticated" signal and a failure pinpoints bad credentials
//! vs. a wrong region/endpoint.

use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};

use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::identity::{self, Whoami};
use crate::region::{region_from_url, RegionMatch};
use crate::render;

/// The region short-name for a resolved endpoint, e.g. `eu2`. Falls back to the
/// endpoint itself for custom / BYOC deployments whose host maps to no known
/// region.
fn region_label(endpoint: &str) -> String {
    match region_from_url(endpoint) {
        RegionMatch::Known(region) => region.to_string(),
        RegionMatch::Unresolved => endpoint.to_string(),
    }
}

/// `cx whoami` — verify credentials and print the caller's identity.
pub async fn run_whoami(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    // whoami checks one profile — the default, or the one named with -p. Fanning
    // out to verify a whole fleet is not the intent, so more than one selected
    // profile is a usage error rather than a silent "first wins".
    if targets.len() > 1 {
        bail!(
            "whoami verifies a single profile — pass at most one --profile (got {})",
            targets.len()
        );
    }
    let target = targets
        .first()
        .expect("build_targets always yields at least one target");

    let whoami = identity::verify_identity(&target.client).await?;
    let region = region_label(&target.cfg.endpoint);

    match output {
        OutputFormat::Json => {
            render::render_json_auto(&[whoami_to_json(&target.profile_name, &whoami, &region)])?
        }
        OutputFormat::Toon => {
            render::render_toon(&[whoami_to_json(&target.profile_name, &whoami, &region)])?
        }
        OutputFormat::Text => print_identity_line(&whoami, &region),
    }

    Ok(())
}

/// One-line human-readable success line, e.g.
/// `✓ authenticated as alice@example.com on team "c4c" (id 53623)`.
fn print_identity_line(whoami: &Whoami, region: &str) {
    let user = whoami.user_name.as_deref().unwrap_or("unknown user");
    let team = match (whoami.team_name.as_deref(), whoami.team_id) {
        (Some(name), Some(id)) => format!(" on team \"{name}\" (id {id})"),
        (Some(name), None) => format!(" on team \"{name}\""),
        (None, Some(id)) => format!(" on team id {id}"),
        (None, None) => String::new(),
    };
    println!("{}", format!("✓ authenticated as {user}{team}").green());
    println!("  {}", format!("region:  {region}").dimmed());
    if let Some(url) = whoami.team_url.as_deref() {
        println!("  {}", format!("console: {url}").dimmed());
    }
}

fn whoami_to_json(profile: &str, whoami: &Whoami, region: &str) -> Value {
    json!({
        "profile": profile,
        "team_id": whoami.team_id,
        "team_name": whoami.team_name,
        "region": region,
        "user_name": whoami.user_name,
        "team_url": whoami.team_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_whoami() -> Whoami {
        Whoami {
            team_id: Some(53623),
            team_name: Some("c4c".to_string()),
            user_name: Some("alice@example.com".to_string()),
            team_url: Some("https://c4c.app.eu2.coralogix.com".to_string()),
        }
    }

    #[test]
    fn json_row_carries_identity_region_and_team_url() {
        let row = whoami_to_json("default", &sample_whoami(), "eu2");
        assert_eq!(row["profile"], "default");
        assert_eq!(row["team_id"], 53623);
        assert_eq!(row["team_name"], "c4c");
        assert_eq!(row["user_name"], "alice@example.com");
        assert_eq!(row["region"], "eu2");
        assert_eq!(row["team_url"], "https://c4c.app.eu2.coralogix.com");
    }

    /// A known API endpoint resolves to its region short-name; a custom / BYOC
    /// endpoint (no known region) falls back to the endpoint itself.
    #[test]
    fn region_label_maps_known_and_falls_back_for_custom() {
        assert_eq!(region_label("https://api.eu2.coralogix.com"), "eu2");
        assert_eq!(
            region_label("https://api.myenv.internal"),
            "https://api.myenv.internal"
        );
    }
}
