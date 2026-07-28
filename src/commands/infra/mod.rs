use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};

pub mod api;

use api::{HealthHistoryEntry, InfraApi, ListResourcesParams, ResourceData, ResourceTypeMapping};

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;

/// JSON key for the source profile when merging multi-profile infra REST rows.
const JSON_KEY_PROFILE: &str = "profile";

/// Scope filter keys accepted by the infrastructure resources API. Validated
/// client-side so a typo fails fast instead of round-tripping for a 400.
const ALLOWED_SCOPE_KEYS: [&str; 3] = ["service", "environment", "team"];

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

    let merged = merge_fan_out(per_profile, |resp| resp.resource_types)?;

    match output {
        OutputFormat::Json | OutputFormat::Agents => {
            let rows: Vec<Value> = merged
                .iter()
                .map(|(profile, m)| type_mapping_to_json(m, include_profile, profile))
                .collect();
            render_machine_rows(output, &rows)?;
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

/// `cx infra resources list` - list resources of a given category and type.
#[allow(clippy::too_many_arguments)]
pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    category: &str,
    resource_type: &str,
    name_filter: Option<&str>,
    scope: &[String],
    start_row: Option<i64>,
    end_row: Option<i64>,
    output: OutputFormat,
) -> Result<()> {
    let category = require_non_empty(category, "--category")?;
    let resource_type = require_non_empty(resource_type, "--type")?;
    let name_filter = name_filter.map(str::trim).filter(|s| !s.is_empty());
    let scope_filters = parse_scope_filters(scope)?;

    eprintln!("{}", "Fetching resources...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| {
        let scope_filters = scope_filters.clone();
        let category = category.to_string();
        let resource_type = resource_type.to_string();
        let name_filter = name_filter.map(String::from);
        async move {
            let api = InfraApi::new(&target.client);
            let params = ListResourcesParams {
                category: &category,
                resource_type: &resource_type,
                name_filter: name_filter.as_deref(),
                scope_filters: &scope_filters,
                start_row,
                end_row,
            };
            Ok(api.list(&params).await?)
        }
    })
    .await;

    let mut total_count: i64 = 0;
    let merged = merge_fan_out(per_profile, |resp| {
        total_count += resp.total_count.unwrap_or(resp.resources.len() as i64);
        resp.resources
    })?;

    match output {
        OutputFormat::Json | OutputFormat::Agents => {
            let rows: Vec<Value> = merged
                .iter()
                .map(|(profile, r)| resource_to_json(r, include_profile, profile))
                .collect();
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            if merged.is_empty() {
                render::print_no_results("No resources found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = merged
                .iter()
                .map(|(profile, r)| {
                    vec![
                        profile.clone(),
                        display_or_dash(r.resource_id.as_deref()),
                        display_or_dash(r.name.as_deref()),
                    ]
                })
                .collect();
            render::render_table(&["Resource ID", "Name"], rows, include_profile);
            eprintln!(
                "{}",
                format!(
                    "Showing {} of {} total resources",
                    merged.len(),
                    total_count
                )
                .dimmed()
            );
        }
    }

    Ok(())
}

/// `cx infra resources health-history <resource-id>` - daily health status samples,
/// oldest first.
pub async fn run_health_history(
    targets: &[Arc<ExecutionTarget>],
    resource_id: &str,
    output: OutputFormat,
) -> Result<()> {
    let resource_id = require_non_empty(resource_id, "resource id")?;

    eprintln!(
        "{}",
        format!("Fetching health history for '{resource_id}'...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let id = resource_id.to_string();

    let per_profile = fan_out(targets, |target| {
        let id = id.clone();
        async move {
            let api = InfraApi::new(&target.client);
            Ok(api.health_history(&id).await?)
        }
    })
    .await;

    let merged = merge_fan_out(per_profile, |resp| resp.health_history)?;

    match output {
        OutputFormat::Json | OutputFormat::Agents => {
            let rows: Vec<Value> = merged
                .iter()
                .map(|(profile, entry)| health_entry_to_json(entry, include_profile, profile))
                .collect();
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            if merged.is_empty() {
                render::print_no_results("No health history found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = merged
                .iter()
                .map(|(profile, entry)| {
                    vec![
                        profile.clone(),
                        display_or_dash(entry.timestamp.as_deref()),
                        display_or_dash(entry.status.as_deref()),
                    ]
                })
                .collect();
            render::render_table(&["Timestamp", "Status"], rows, include_profile);
        }
    }

    Ok(())
}

/// `cx infra resources raw-data <resource-id>` - the raw resource document.
pub async fn run_raw_data(
    targets: &[Arc<ExecutionTarget>],
    resource_id: &str,
    output: OutputFormat,
) -> Result<()> {
    let resource_id = require_non_empty(resource_id, "resource id")?;

    eprintln!(
        "{}",
        format!("Fetching raw resource data for '{resource_id}'...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let id = resource_id.to_string();

    let per_profile = fan_out(targets, |target| {
        let id = id.clone();
        async move {
            let api = InfraApi::new(&target.client);
            Ok(api.raw_data(&id).await?)
        }
    })
    .await;

    // Merge - one document per profile; a 200 with null raw data means the
    // document is cleanly missing, so note it on stderr and move on.
    let target_count = per_profile.len();
    let mut error_count = 0usize;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => match resp.raw_data {
                Some(mut doc) => {
                    if include_profile {
                        render::tag_get_result(&mut doc, &profile);
                    }
                    all_results.push(doc);
                }
                None => eprintln!(
                    "{}",
                    format!("no raw data for this resource in profile '{profile}'").yellow()
                ),
            },
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
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => render::render_agents(&all_results)?,
        OutputFormat::Text => {
            render::render_get_text(&all_results, include_profile, "No raw data found.", None)?;
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Flattens fan-out results into `(profile, item)` pairs, printing per-profile
/// errors to stderr (non-fatal) and erroring only when **all** profiles fail,
/// so CI/scripts see a non-zero exit when nothing succeeded.
fn merge_fan_out<R, T>(
    per_profile: Vec<(String, Result<R>)>,
    mut extract: impl FnMut(R) -> Vec<T>,
) -> Result<Vec<(String, T)>> {
    let target_count = per_profile.len();
    let mut error_count = 0usize;
    let mut merged: Vec<(String, T)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for item in extract(resp) {
                    merged.push((profile.clone(), item));
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
    Ok(merged)
}

/// Renders merged JSON rows for the two machine formats. Callers handle
/// `OutputFormat::Text` themselves, so it is rejected here.
fn render_machine_rows(output: OutputFormat, rows: &[Value]) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json(rows),
        OutputFormat::Agents => render::render_agents(rows),
        OutputFormat::Text => bail!("render_machine_rows called with text output"),
    }
}

/// Trims a required string input and rejects it when nothing remains, so an
/// empty `--category ""` fails fast instead of sending an empty query
/// parameter to the API.
fn require_non_empty<'v>(value: &'v str, field_name: &str) -> Result<&'v str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{field_name} must not be empty");
    }
    Ok(trimmed)
}

/// Parses repeatable `--scope key=value` flags and validates keys against
/// [`ALLOWED_SCOPE_KEYS`].
fn parse_scope_filters(scope: &[String]) -> Result<Vec<(String, String)>> {
    scope
        .iter()
        .map(|raw| {
            let Some((key, value)) = raw.split_once('=') else {
                bail!("invalid --scope '{raw}': expected key=value");
            };
            let key = key.trim();
            let value = value.trim();
            if !ALLOWED_SCOPE_KEYS.contains(&key) {
                bail!(
                    "unknown --scope key '{key}'; allowed keys: {}",
                    ALLOWED_SCOPE_KEYS.join(", ")
                );
            }
            if value.is_empty() {
                bail!("invalid --scope '{raw}': value must not be empty");
            }
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}

/// Builds one resource row as JSON for `json` / `agents` output after fan-out.
fn resource_to_json(item: &ResourceData, include_profile: bool, profile: &str) -> Value {
    let v = json!({
        "resource_id": item.resource_id,
        "name": item.name,
        "columns": item.columns,
    });
    tag_profile(v, include_profile, profile)
}

/// Injects the profile key into a JSON row when `include_profile` is true
/// (multiple `--profile`), so merged arrays stay attributable per account;
/// text mode uses a separate table path.
fn tag_profile(mut v: Value, include_profile: bool, profile: &str) -> Value {
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

/// Builds one health-history row as JSON for `json` / `agents` output after fan-out.
fn health_entry_to_json(item: &HealthHistoryEntry, include_profile: bool, profile: &str) -> Value {
    let v = json!({
        "timestamp": item.timestamp,
        "status": item.status,
    });
    tag_profile(v, include_profile, profile)
}

/// Builds one resource-type row as JSON for `json` / `agents` output after fan-out.
fn type_mapping_to_json(item: &ResourceTypeMapping, include_profile: bool, profile: &str) -> Value {
    let v = json!({
        "category": item.category_type.as_ref().and_then(|c| c.category.clone()),
        "type": item.category_type.as_ref().and_then(|c| c.type_name.clone()),
        "resource_type": item.resource_type,
        "label": item.label,
    });
    tag_profile(v, include_profile, profile)
}

fn display_or_dash(value: Option<&str>) -> String {
    value.filter(|s| !s.is_empty()).unwrap_or("-").to_string()
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_non_empty_trims_and_accepts_values() {
        assert_eq!(require_non_empty(" Hosts ", "--category").unwrap(), "Hosts");
    }

    #[test]
    fn require_non_empty_rejects_empty_and_whitespace() {
        let err = require_non_empty("", "--category").unwrap_err();
        assert!(err.to_string().contains("--category must not be empty"));

        let err = require_non_empty("   ", "resource id").unwrap_err();
        assert!(err.to_string().contains("resource id must not be empty"));
    }

    #[test]
    fn parse_scope_filters_accepts_allowed_keys() {
        let scope = vec![
            "service=checkout".to_string(),
            "environment=prod".to_string(),
            "team=platform".to_string(),
        ];
        let filters = parse_scope_filters(&scope).unwrap();
        assert_eq!(
            filters,
            vec![
                ("service".to_string(), "checkout".to_string()),
                ("environment".to_string(), "prod".to_string()),
                ("team".to_string(), "platform".to_string()),
            ]
        );
    }

    #[test]
    fn parse_scope_filters_trims_whitespace() {
        let scope = vec![" service = checkout ".to_string()];
        let filters = parse_scope_filters(&scope).unwrap();
        assert_eq!(
            filters,
            vec![("service".to_string(), "checkout".to_string())]
        );
    }

    #[test]
    fn parse_scope_filters_rejects_missing_equals() {
        let err = parse_scope_filters(&["service".to_string()]).unwrap_err();
        assert!(err.to_string().contains("expected key=value"));
    }

    #[test]
    fn parse_scope_filters_rejects_unknown_key() {
        let err = parse_scope_filters(&["region=us-east-1".to_string()]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown --scope key 'region'"));
        assert!(msg.contains("service, environment, team"));
    }

    #[test]
    fn parse_scope_filters_rejects_empty_value() {
        let err = parse_scope_filters(&["service=".to_string()]).unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn parse_scope_filters_empty_input_yields_no_filters() {
        assert!(parse_scope_filters(&[]).unwrap().is_empty());
    }

    #[test]
    fn tag_profile_inserts_key_only_when_multi_profile() {
        let tagged = tag_profile(json!({"a": 1}), true, "prod");
        assert_eq!(tagged["profile"], "prod");

        let untagged = tag_profile(json!({"a": 1}), false, "prod");
        assert!(untagged.get("profile").is_none());
    }

    #[test]
    fn display_or_dash_falls_back_on_none_and_empty() {
        assert_eq!(display_or_dash(Some("value")), "value");
        assert_eq!(display_or_dash(Some("")), "-");
        assert_eq!(display_or_dash(None), "-");
    }
}
