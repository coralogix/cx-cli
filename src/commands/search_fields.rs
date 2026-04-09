use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use tabled::{Table, Tabled};

use crate::api::schema_store::{semantic_field_lookup, SemanticFieldResult};
use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};

#[derive(Tabled)]
struct FieldRow {
    #[tabled(rename = "Profile")]
    profile: String,
    #[tabled(rename = "DataPrime path")]
    dataprime_path: String,
    #[tabled(rename = "Description")]
    description: String,
    #[tabled(rename = "Similarity")]
    similarity: String,
}

#[derive(Tabled)]
struct FieldRowSingle {
    #[tabled(rename = "DataPrime path")]
    dataprime_path: String,
    #[tabled(rename = "Description")]
    description: String,
    #[tabled(rename = "Similarity")]
    similarity: String,
}

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
            let team_id = t.cfg.team_id.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "cgx-team-id is required for search-fields.\n\
                     Run `cx profiles add` and enter your Coralogix team ID."
                )
            })?;
            semantic_field_lookup(&t.cfg.endpoint, &t.cfg.api_key, team_id, &tx, &ds, limit).await
        }
    })
    .await;

    let mut all_results: Vec<(String, SemanticFieldResult)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(results) => {
                for r in results {
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
            println!("{}", serde_json::to_string_pretty(&json_rows)?);
        }
        OutputFormat::Text => {
            if all_results.is_empty() {
                println!("{}", "No matching fields found.".yellow());
                return Ok(());
            }
            if include_profile {
                let rows: Vec<FieldRow> = all_results
                    .iter()
                    .map(|(profile, r)| FieldRow {
                        profile: profile.clone(),
                        dataprime_path: r.dataprime_path.clone(),
                        description: r.description.clone(),
                        similarity: format!("{:.3}", r.similarity),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            } else {
                let rows: Vec<FieldRowSingle> = all_results
                    .iter()
                    .map(|(_, r)| FieldRowSingle {
                        dataprime_path: r.dataprime_path.clone(),
                        description: r.description.clone(),
                        similarity: format!("{:.3}", r.similarity),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            }
        }
    }

    Ok(())
}
