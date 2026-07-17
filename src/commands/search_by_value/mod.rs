use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::Value;

pub mod api;

use crate::commands::dashboards::profiled_api_row_to_json;
use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;
use api::SearchByValueResult;

pub async fn run(
    targets: &[Arc<ExecutionTarget>],
    query: &str,
    dataset: &str,
    limit: u32,
    offset: u32,
    output: OutputFormat,
) -> Result<()> {
    if query.trim().is_empty() {
        bail!("query text cannot be empty");
    }

    eprintln!(
        "{}",
        format!("Searching {dataset} values for: {query:?}…").dimmed()
    );

    let include_profile = targets.len() > 1;
    let target_count = targets.len();
    let query_owned = query.to_string();
    let dataset_owned = dataset.to_string();
    let per_profile = fan_out(targets, |target| {
        let query_clone = query_owned.clone();
        let dataset_clone = dataset_owned.clone();
        async move {
            api::search_by_value(&target.client, &query_clone, &dataset_clone, limit, offset)
                .await
                .map_err(Into::into)
        }
    })
    .await;

    let mut error_count = 0usize;
    let mut all_results: Vec<(String, SearchByValueResult)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for r in resp.matches {
                    all_results.push((profile.clone(), r));
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

    let json_rows: Vec<Value> = all_results
        .iter()
        .map(|(profile, r)| {
            profiled_api_row_to_json(profile, r, include_profile, "search-by-value")
        })
        .collect::<Result<Vec<_>>>()?;

    match output {
        OutputFormat::Json => render::render_json(&json_rows)?,
        OutputFormat::Yaml => render::render_yaml(&json_rows)?,
        OutputFormat::Agents => render::render_agents(&json_rows)?,
        OutputFormat::Text => {
            if all_results.is_empty() {
                render::print_no_results("No matching values found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_results
                .iter()
                .map(|(profile, r)| {
                    vec![
                        profile.clone(),
                        r.key_matched.clone(),
                        r.value.clone(),
                        format!("{:.3}", r.similarity_score),
                    ]
                })
                .collect();
            render::render_table(
                &["Key Matched", "Value", "Similarity"],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}
