pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::DatasetsApi;

pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    filter: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching datasets...".dimmed());

    let include_profile = targets.len() > 1;

    let filter = filter.map(|s| s.to_string());
    let per_profile = fan_out(targets, |t| {
        let filter = filter.clone();
        async move {
            let api = DatasetsApi::new(&t.client);
            Ok(api.list(filter.as_deref()).await?)
        }
    })
    .await;

    let mut json_rows: Vec<Value> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for ds in resp.datasets {
            let mut row = json!({
                "teamId": ds.team_id,
                "dataspace": ds.dataspace,
                "dataset": ds.dataset,
                "version": ds.version,
                "pillar": ds.pillar,
                "entityType": ds.entity_type,
            });
            if include_profile {
                row.as_object_mut()
                    .unwrap()
                    .insert("profile".to_string(), Value::String(profile.clone()));
            }
            json_rows.push(row);
            table_rows.push(vec![
                profile.clone(),
                ds.source(),
                ds.pillar.clone().unwrap_or_else(|| "-".to_string()),
                ds.entity_type.clone().unwrap_or_else(|| "-".to_string()),
                ds.version
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            ]);
        }
    }

    match output {
        OutputFormat::Json => render::render_json(&json_rows)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&json_rows)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if table_rows.is_empty() {
                render::print_no_results("No datasets found.");
                return Ok(());
            }
            render::render_table(
                &["Source", "Pillar", "Entity Type", "Version"],
                table_rows,
                include_profile,
            );
        }
    }

    Ok(())
}
