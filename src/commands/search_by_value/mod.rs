use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;

pub mod api;

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
    eprintln!(
        "{}",
        format!("Searching {dataset} values for: {query:?}…").dimmed()
    );

    let include_profile = targets.len() > 1;
    let query = query.to_string();
    let dataset = dataset.to_string();
    let per_profile = fan_out(targets, |t| {
        let q = query.clone();
        let ds = dataset.clone();
        async move {
            api::search_by_value(&t.client, &q, &ds, limit, offset)
                .await
                .map_err(Into::into)
        }
    })
    .await;

    let mut all_results: Vec<(String, SearchByValueResult)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for r in resp.matches {
                    all_results.push((profile.clone(), r));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json | OutputFormat::Agents => {
            let json_rows: Vec<Value> = all_results
                .iter()
                .map(|(profile, r)| {
                    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
                    if include_profile {
                        if let Value::Object(ref mut m) = v {
                            m.insert("profile".to_string(), Value::String(profile.clone()));
                        }
                    }
                    v
                })
                .collect();
            render::render_json(&json_rows)?;
        }
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
