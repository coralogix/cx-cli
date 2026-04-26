use std::io::Read;
use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use rand::RngExt;
use serde_json::{json, Value};
use tabled::{Table, Tabled};
use toon_format::encode_default as toon_encode;

use crate::api::dashboards::{DashboardFolderItem, DashboardsApi};
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
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&all_rows)?);
        }
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_rows).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
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
                        folder: item
                            .folder
                            .as_ref()
                            .and_then(|f| f.name.clone())
                            .unwrap_or_default(),
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
                        folder: item
                            .folder
                            .as_ref()
                            .and_then(|f| f.name.clone())
                            .unwrap_or_default(),
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
                println!();
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}

// ── Create ────────────────────────────────────────────────────────────────────

#[derive(Tabled)]
struct CreatedRow {
    #[tabled(rename = "Profile")]
    profile: String,
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
}

/// Generate a random hex string for the `requestId` envelope field.
fn new_request_id() -> String {
    let mut rng = rand::rng();
    (0..16)
        .map(|_| format!("{:02x}", rng.random::<u8>()))
        .collect()
}

/// Read a JSON payload from a file path or stdin (when `from_file == "-"`),
/// and normalize it into the inner `dashboard` object expected by the
/// `CreateDashboard` API. Accepts either the bare dashboard JSON or the
/// `{ "dashboard": {...} }` wrapper form.
fn read_dashboard_body(from_file: &str) -> Result<Value> {
    let raw = if from_file == "-" {
        eprintln!("{}", "Reading dashboard definition from stdin...".dimmed());
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading dashboard definition from {from_file}...").dimmed()
        );
        std::fs::read_to_string(from_file)?
    };

    let parsed: Value = serde_json::from_str(&raw)?;

    // Allow either a bare dashboard doc or a pre-wrapped request payload.
    let dashboard = if parsed.get("dashboard").is_some() {
        parsed
            .get("dashboard")
            .cloned()
            .unwrap_or_else(|| json!({}))
    } else {
        parsed
    };

    if !dashboard.is_object() {
        bail!("Dashboard JSON must be a JSON object (got {})", dashboard);
    }
    if dashboard.get("layout").is_none() {
        bail!(
            "Dashboard JSON is missing required 'layout' field. See `cx dashboards create --help`."
        );
    }

    Ok(dashboard)
}

pub async fn run_create(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    folder_id: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let mut dashboard = read_dashboard_body(from_file)?;

    // Inject folder assignment if the caller provided one.
    if let Some(folder) = folder_id {
        if let Value::Object(ref mut m) = dashboard {
            m.insert(
                "folderId".to_string(),
                json!({ "value": folder.to_string() }),
            );
        }
    }

    let name = dashboard
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("<unnamed>")
        .to_string();

    eprintln!("{}", format!("Creating dashboard '{name}'...").dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let dashboard = dashboard.clone();
        async move {
            let body = json!({
                "requestId": new_request_id(),
                "dashboard": dashboard,
            });
            let api = DashboardsApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<(String, String, Value)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(mut resp) => {
                let created_id = resp
                    .get("dashboardId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        resp.get("id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .or_else(|| {
                        resp.get("dashboard")
                            .and_then(|d| d.get("id"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| "unknown".to_string());

                if include_profile {
                    if let Value::Object(ref mut m) = resp {
                        m.insert("_profile".to_string(), Value::String(profile.clone()));
                    }
                }
                all_results.push((profile, created_id, resp));
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => {
            if all_results.len() == 1 {
                println!("{}", serde_json::to_string_pretty(&all_results[0].2)?);
            } else {
                let vals: Vec<&Value> = all_results.iter().map(|(_, _, v)| v).collect();
                println!("{}", serde_json::to_string_pretty(&vals)?);
            }
        }
        OutputFormat::Agents => {
            let vals: Vec<&Value> = all_results.iter().map(|(_, _, v)| v).collect();
            let toon =
                toon_encode(&vals).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_results.is_empty() {
                // Per-profile errors already surfaced via eprintln! above.
                return Ok(());
            }
            if include_profile {
                let rows: Vec<CreatedRow> = all_results
                    .iter()
                    .map(|(profile, id, _)| CreatedRow {
                        profile: profile.clone(),
                        id: id.clone(),
                        name: name.clone(),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            } else {
                let (_, id, _) = &all_results[0];
                println!(
                    "{}",
                    format!("Created dashboard '{name}' (ID: {id})")
                        .green()
                        .bold()
                );
            }
        }
    }

    Ok(())
}

// ── Folders ───────────────────────────────────────────────────────────────────

#[derive(Tabled)]
struct FolderRow {
    #[tabled(rename = "Profile")]
    profile: String,
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Parent ID")]
    parent_id: String,
}

#[derive(Tabled)]
struct FolderRowSingle {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Parent ID")]
    parent_id: String,
}

fn folder_item_to_json(item: &DashboardFolderItem, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": item.id_str(),
        "name": item.name,
        "parent_id": item.parent_id_str(),
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

pub async fn run_folders_list(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching dashboard folders...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = DashboardsApi::new(&t.client);
        Ok(api.folders().await?)
    })
    .await;

    let mut all_rows: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, DashboardFolderItem)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for item in resp.folders {
                    all_rows.push(folder_item_to_json(&item, include_profile, &profile));
                    all_items.push((profile.clone(), item));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&all_rows)?);
        }
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_rows).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                println!("{}", "No dashboard folders found.".yellow());
                return Ok(());
            }
            if include_profile {
                let rows: Vec<FolderRow> = all_items
                    .iter()
                    .map(|(profile, item)| FolderRow {
                        profile: profile.clone(),
                        id: item.id_str().unwrap_or("").to_string(),
                        name: item.name.clone().unwrap_or_default(),
                        parent_id: item.parent_id_str().unwrap_or("").to_string(),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            } else {
                let rows: Vec<FolderRowSingle> = all_items
                    .iter()
                    .map(|(_, item)| FolderRowSingle {
                        id: item.id_str().unwrap_or("").to_string(),
                        name: item.name.clone().unwrap_or_default(),
                        parent_id: item.parent_id_str().unwrap_or("").to_string(),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            }
        }
    }

    Ok(())
}

#[derive(Tabled)]
struct FolderCreatedRow {
    #[tabled(rename = "Profile")]
    profile: String,
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
}

pub async fn run_folders_create(
    targets: &[Arc<ExecutionTarget>],
    name: &str,
    parent_id: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Creating dashboard folder '{name}'...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let name_owned = name.to_string();
    let parent_id_owned = parent_id.map(|s| s.to_string());

    let per_profile = fan_out(targets, |t| {
        let name = name_owned.clone();
        let parent_id = parent_id_owned.clone();
        async move {
            let mut folder = json!({ "name": name });
            if let (Some(p), Value::Object(ref mut m)) = (parent_id.as_ref(), &mut folder) {
                m.insert("parentId".to_string(), Value::String(p.clone()));
            }
            let body = json!({
                "requestId": new_request_id(),
                "folder": folder,
            });
            let api = DashboardsApi::new(&t.client);
            Ok(api.folders_create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<(String, String, Value)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(mut resp) => {
                let created_id = resp
                    .get("folderId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        resp.get("id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| "unknown".to_string());
                if include_profile {
                    if let Value::Object(ref mut m) = resp {
                        m.insert("_profile".to_string(), Value::String(profile.clone()));
                    }
                }
                all_results.push((profile, created_id, resp));
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => {
            if all_results.len() == 1 {
                println!("{}", serde_json::to_string_pretty(&all_results[0].2)?);
            } else {
                let vals: Vec<&Value> = all_results.iter().map(|(_, _, v)| v).collect();
                println!("{}", serde_json::to_string_pretty(&vals)?);
            }
        }
        OutputFormat::Agents => {
            let vals: Vec<&Value> = all_results.iter().map(|(_, _, v)| v).collect();
            let toon =
                toon_encode(&vals).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_results.is_empty() {
                return Ok(());
            }
            if include_profile {
                let rows: Vec<FolderCreatedRow> = all_results
                    .iter()
                    .map(|(profile, id, _)| FolderCreatedRow {
                        profile: profile.clone(),
                        id: id.clone(),
                        name: name.to_string(),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            } else {
                let (_, id, _) = &all_results[0];
                println!(
                    "{}",
                    format!("Created folder '{name}' (ID: {id})").green().bold()
                );
            }
        }
    }

    Ok(())
}
