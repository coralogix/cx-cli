use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;

use crate::commands::dataprime::semantic_search::{semantic_field_lookup, SemanticFieldResult};
use crate::config::OutputFormat;
use crate::execution::{collect_successes, fan_out, ExecutionTarget};
use crate::render;

pub async fn run(
    targets: &[Arc<ExecutionTarget>],
    text: &str,
    dataset: &str,
    limit: u32,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Searching {dataset} fields for: {text:?}…").dimmed()
    );

    let include_profile = targets.len() > 1;
    let text = text.to_string();
    let dataset = dataset.to_string();
    let per_profile = fan_out(targets, |t| {
        let tx = text.clone();
        let ds = dataset.clone();
        async move {
            semantic_field_lookup(&t.client, &tx, &ds, limit)
                .await
                .map_err(Into::into)
        }
    })
    .await;

    let mut all_results: Vec<(String, SemanticFieldResult)> = Vec::new();
    for (profile, results) in collect_successes(per_profile)? {
        for r in results {
            all_results.push((profile.clone(), r));
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
                render::print_no_results("No matching fields found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_results
                .iter()
                .map(|(profile, r)| {
                    vec![
                        profile.clone(),
                        r.dataprime_path.clone(),
                        r.description.clone(),
                        format!("{:.3}", r.similarity),
                    ]
                })
                .collect();
            render::render_table(
                &["DataPrime path", "Description", "Similarity"],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}
