use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use tabled::{Table, Tabled};
use toon_format::encode_default as toon_encode;

use crate::api::dashboards::DashboardsApi;
use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};

// ── Text-output row types ─────────────────────────────────────────────────────

#[derive(Tabled)]
struct CatalogRow {
    #[tabled(rename = "Profile")]
    profile: String,
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Folder")]
    folder: String,
    #[tabled(rename = "Updated")]
    updated: String,
    #[tabled(rename = "Pinned")]
    pinned: String,
    #[tabled(rename = "Locked")]
    locked: String,
}

#[derive(Tabled)]
struct CatalogRowSingle {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Folder")]
    folder: String,
    #[tabled(rename = "Updated")]
    updated: String,
    #[tabled(rename = "Pinned")]
    pinned: String,
    #[tabled(rename = "Locked")]
    locked: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bool_display(v: Option<bool>) -> String {
    match v {
        Some(true) => "yes".to_string(),
        Some(false) => "no".to_string(),
        None => "-".to_string(),
    }
}

fn catalog_item_to_json(item: &crate::api::dashboards::DashboardCatalogItem, include_profile: bool, profile: &str) -> Value {
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

pub async fn run_catalog(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching dashboard catalog...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        async move {
            let api = DashboardsApi::new(&t.client);
            Ok(api.catalog().await?)
        }
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
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&all_rows)?);
        }
        OutputFormat::Agents => {
            let toon = toon_encode(&all_rows)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                println!("{}", "No dashboards found.".yellow());
                return Ok(());
            }
            if include_profile {
                let rows: Vec<CatalogRow> = all_items
                    .iter()
                    .map(|(profile, item)| CatalogRow {
                        profile: profile.clone(),
                        id: item.id.clone().unwrap_or_default(),
                        name: item.name.clone().unwrap_or_default(),
                        folder: item.folder.as_ref().and_then(|f| f.name.clone()).unwrap_or_default(),
                        updated: item.update_time.clone().unwrap_or_default(),
                        pinned: bool_display(item.is_pinned),
                        locked: bool_display(item.is_locked),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            } else {
                let rows: Vec<CatalogRowSingle> = all_items
                    .iter()
                    .map(|(_, item)| CatalogRowSingle {
                        id: item.id.clone().unwrap_or_default(),
                        name: item.name.clone().unwrap_or_default(),
                        folder: item.folder.as_ref().and_then(|f| f.name.clone()).unwrap_or_default(),
                        updated: item.update_time.clone().unwrap_or_default(),
                        pinned: bool_display(item.is_pinned),
                        locked: bool_display(item.is_locked),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            }
        }
    }

    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    dashboard_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching dashboard {dashboard_id}...").dimmed());

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
                    if let Value::Object(ref mut m) = val {
                        m.insert("_profile".to_string(), Value::String(profile.clone()));
                    }
                }
                all_results.push(val);
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    // Render
    match output {
        OutputFormat::Json => {
            if all_results.len() == 1 {
                println!("{}", serde_json::to_string_pretty(&all_results[0])?);
            } else {
                println!("{}", serde_json::to_string_pretty(&all_results)?);
            }
        }
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_results.is_empty() {
                println!("{}", "Dashboard not found.".yellow());
                return Ok(());
            }
            for val in &all_results {
                if include_profile {
                    if let Some(p) = val.get("_profile").and_then(|v| v.as_str()) {
                        println!("{}", format!("[{p}]").dimmed());
                    }
                }
                let name = val.get("name").or_else(|| {
                    val.get("dashboard").and_then(|d| d.get("name"))
                }).and_then(|v| v.as_str()).unwrap_or("-");
                let id = val.get("id").or_else(|| {
                    val.get("dashboard").and_then(|d| d.get("id"))
                }).and_then(|v| v.as_str()).unwrap_or("-");
                let desc = val.get("description").or_else(|| {
                    val.get("dashboard").and_then(|d| d.get("description"))
                }).and_then(|v| v.as_str()).unwrap_or("");

                println!("{}: {}", "Name".bold(), name);
                println!("{}:   {}", "ID".bold(), id);
                if !desc.is_empty() {
                    println!("{}: {}", "Desc".bold(), desc);
                }
                println!();
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}
