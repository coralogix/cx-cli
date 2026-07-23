use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};

pub mod api;

use api::{InfraApi, ResourceTypeMapping};

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;

/// JSON key for the source profile when merging multi-profile infra REST rows.
const JSON_KEY_PROFILE: &str = "profile";

// ── Subcommand runners ────────────────────────────────────────────────────────

/// `cx infra resources types` - list the available resource type mappings.
pub async fn run_types(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching available resource types...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| async move {
        let api = InfraApi::new(&target.client);
        Ok(api.available_types().await?)
    })
    .await;

    let target_count = per_profile.len();
    let mut error_count = 0usize;
    let mut merged: Vec<(String, ResourceTypeMapping)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for mapping in resp.resource_types {
                    merged.push((profile.clone(), mapping));
                }
            }
            Err(e) => {
                error_count += 1;
                eprintln!("{}", format!("error from profile '{profile}': {e:#}").red());
            }
        }
    }
    if target_count > 0 && error_count == target_count {
        bail!("all profiles returned errors; see above for details");
    }

    match output {
        OutputFormat::Json | OutputFormat::Agents => {
            let rows: Vec<Value> = merged
                .iter()
                .map(|(profile, m)| type_mapping_to_json(m, include_profile, profile))
                .collect();
            if output == OutputFormat::Json {
                render::render_json(&rows)?;
            } else {
                render::render_agents(&rows)?;
            }
        }
        OutputFormat::Text => {
            if merged.is_empty() {
                render::print_no_results("No resource types found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = merged
                .iter()
                .map(|(profile, m)| {
                    vec![
                        profile.clone(),
                        display_or_dash(
                            m.category_type.as_ref().and_then(|c| c.category.as_deref()),
                        ),
                        display_or_dash(
                            m.category_type
                                .as_ref()
                                .and_then(|c| c.type_name.as_deref()),
                        ),
                        display_or_dash(m.resource_type.as_deref()),
                        display_or_dash(m.label.as_deref()),
                    ]
                })
                .collect();
            render::render_table(
                &["Category", "Type", "Resource Type", "Label"],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Builds one resource-type row as JSON for `json` / `agents` output after fan-out.
///
/// When `include_profile` is true (multiple `--profile`), injects the profile key so
/// merged arrays stay attributable per account; text mode uses a separate table path.
fn type_mapping_to_json(item: &ResourceTypeMapping, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "category": item.category_type.as_ref().and_then(|c| c.category.clone()),
        "type": item.category_type.as_ref().and_then(|c| c.type_name.clone()),
        "resource_type": item.resource_type,
        "label": item.label,
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert(
                JSON_KEY_PROFILE.to_string(),
                Value::String(profile.to_string()),
            );
        }
    }
    v
}

fn display_or_dash(value: Option<&str>) -> String {
    value.filter(|s| !s.is_empty()).unwrap_or("-").to_string()
}
