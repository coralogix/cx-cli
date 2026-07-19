pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;
use api::DatasetsApi;

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching datasets...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = DatasetsApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    let mut all_results: Vec<(String, Value)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(listing) => {
                let mut val = listing.to_json_value();
                if include_profile {
                    if let Value::Object(ref mut map) = val {
                        map.insert("profile".into(), Value::String(profile.clone()));
                    }
                }
                all_results.push((profile, val));
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => {
            let rows: Vec<Value> = all_results.into_iter().map(|(_, v)| v).collect();
            render::render_json_auto(&rows)?;
        }
        OutputFormat::Agents => {
            let rows: Vec<Value> = all_results.into_iter().map(|(_, v)| v).collect();
            let toon =
                toon_encode(&rows).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_results.is_empty() {
                render::print_no_results("No datasets found.");
                return Ok(());
            }

            let mut rows: Vec<Vec<String>> = Vec::new();
            for (profile, val) in &all_results {
                let datasets = val
                    .get("datasets")
                    .and_then(|d| d.as_array())
                    .cloned()
                    .unwrap_or_default();
                for ds in datasets {
                    let dtype = ds
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string();
                    let source = ds
                        .get("source")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string();
                    let description = ds
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string();
                    let enabled = if dtype == "system" {
                        render::bool_display(ds.get("queryEnabled").and_then(|v| v.as_bool()))
                    } else {
                        render::bool_display(ds.get("writeEnabled").and_then(|v| v.as_bool()))
                    };
                    rows.push(vec![profile.clone(), dtype, source, description, enabled]);
                }
            }

            if rows.is_empty() {
                // Still print always-available sources summary when only logs/spans exist.
                for (_, val) in &all_results {
                    if let Some(sources) =
                        val.get("alwaysAvailableSources").and_then(|v| v.as_array())
                    {
                        let joined: Vec<&str> = sources.iter().filter_map(|v| v.as_str()).collect();
                        println!(
                            "{}",
                            format!("Always available: {}", joined.join(", ")).dimmed()
                        );
                    }
                }
                render::print_no_results("No system or user-defined datasets found.");
                return Ok(());
            }

            render::render_table(
                &["Type", "Source", "Description", "Enabled"],
                rows,
                include_profile,
            );

            for (_, val) in &all_results {
                if val.get("truncated").and_then(|v| v.as_bool()) == Some(true) {
                    eprintln!(
                        "{}",
                        "Result truncated to 50 system + 50 user datasets.".yellow()
                    );
                    break;
                }
            }
        }
    }

    Ok(())
}
