use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};

use crate::api::dashboards::DashboardsApi;
use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn catalog_item_to_json(
    item: &crate::api::dashboards::DashboardCatalogItem,
    include_profile: bool,
    profile: &str,
) -> Value {
    let mut v = json!({
        "id": item.id,
        "name": item.name,
        "description": item.description,
        "slug_name": item.slug_name,
        "create_time": item.create_time,
        "update_time": item.update_time,
        "is_default": item.is_default,
        "is_pinned": item.is_pinned,
        "is_locked": item.is_locked,
        "folder": item.folder.as_ref().map(|f| json!({
            "id": f.id,
            "name": f.name,
            "parent_id": f.parent_id,
        })),
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_catalog(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching dashboard catalog...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = DashboardsApi::new(&t.client);
        Ok(api.catalog().await?)
    })
    .await;

    // Merge
    let mut all_rows: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, crate::api::dashboards::DashboardCatalogItem)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for item in resp.items {
                    all_rows.push(catalog_item_to_json(&item, include_profile, &profile));
                    all_items.push((profile.clone(), item));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    // Render
    match output {
        OutputFormat::Json => render::render_json(&all_rows)?,
        OutputFormat::Agents => render::render_agents(&all_rows)?,
        OutputFormat::Text => {
            if all_items.is_empty() {
                render::print_no_results("No dashboards found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, item)| {
                    vec![
                        profile.clone(),
                        item.id.clone().unwrap_or_default(),
                        item.name.clone().unwrap_or_default(),
                        item.folder
                            .as_ref()
                            .and_then(|f| f.name.clone())
                            .unwrap_or_default(),
                        item.update_time.clone().unwrap_or_default(),
                        render::bool_display(item.is_pinned),
                        render::bool_display(item.is_locked),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Folder", "Updated", "Pinned", "Locked"],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    dashboard_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching dashboard {dashboard_id}...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let id = dashboard_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = DashboardsApi::new(&t.client);
            Ok(api.get(&id).await?)
        }
    })
    .await;

    // Merge
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(mut val) => {
                if include_profile {
                    render::tag_get_result(&mut val, &profile);
                }
                all_results.push(val);
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    // Render
    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => render::render_agents(&all_results)?,
        OutputFormat::Text => {
            render::render_get_text(
                &all_results,
                include_profile,
                "Dashboard not found.",
                Some(&|val| {
                    let name = val
                        .get("name")
                        .or_else(|| val.get("dashboard").and_then(|d| d.get("name")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let id = val
                        .get("id")
                        .or_else(|| val.get("dashboard").and_then(|d| d.get("id")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let desc = val
                        .get("description")
                        .or_else(|| val.get("dashboard").and_then(|d| d.get("description")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    println!("{}: {}", "Name".bold(), name);
                    println!("{}:   {}", "ID".bold(), id);
                    if !desc.is_empty() {
                        println!("{}: {}", "Desc".bold(), desc);
                    }
                }),
            )?;
        }
    }

    Ok(())
}
