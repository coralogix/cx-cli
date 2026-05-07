use std::io::Read;
use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use rand::RngExt;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

pub mod api;

use api::{DashboardFolderItem, DashboardsApi};

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn catalog_item_to_json(
    item: &api::DashboardCatalogItem,
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
    let mut all_items: Vec<(String, api::DashboardCatalogItem)> = Vec::new();
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
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_rows).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
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
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
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

// ── Create ────────────────────────────────────────────────────────────────────

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
                    render::tag_get_result(&mut resp, &profile);
                }
                all_results.push((profile, created_id, resp));
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => {
            let vals: Vec<Value> = all_results.iter().map(|(_, _, v)| v.clone()).collect();
            render::render_json_auto(&vals)?;
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
                let rows: Vec<Vec<String>> = all_results
                    .iter()
                    .map(|(profile, id, _)| vec![profile.clone(), id.clone(), name.clone()])
                    .collect();
                render::render_table(&["ID", "Name"], rows, true);
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

// ── Replace ──────────────────────────────────────────────────────────────────

pub async fn run_replace(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let dashboard = read_dashboard_body(from_file)?;

    let id_field = dashboard
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if id_field.is_none() {
        bail!(
            "Dashboard JSON is missing required 'id' field. \
             Use `cx dashboards get <id> -o json` to fetch the full definition, \
             then pass the edited JSON to `cx dashboards replace`."
        );
    }

    let name = dashboard
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("<unnamed>")
        .to_string();

    eprintln!("{}", format!("Replacing dashboard '{name}'...").dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let dashboard = dashboard.clone();
        async move {
            let body = json!({
                "requestId": new_request_id(),
                "dashboard": dashboard,
            });
            let api = DashboardsApi::new(&t.client);
            Ok(api.replace(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<(String, String, Value)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(mut resp) => {
                let replaced_id = resp
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
                    render::tag_get_result(&mut resp, &profile);
                }
                all_results.push((profile, replaced_id, resp));
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => {
            let vals: Vec<Value> = all_results.iter().map(|(_, _, v)| v.clone()).collect();
            render::render_json_auto(&vals)?;
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
                let rows: Vec<Vec<String>> = all_results
                    .iter()
                    .map(|(profile, id, _)| vec![profile.clone(), id.clone(), name.clone()])
                    .collect();
                render::render_table(&["ID", "Name"], rows, true);
            } else {
                let (_, id, _) = &all_results[0];
                println!(
                    "{}",
                    format!("Replaced dashboard '{name}' (ID: {id})")
                        .green()
                        .bold()
                );
            }
        }
    }

    Ok(())
}

// ── Delete ────────────────────────────────────────────────────────────────────

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    eprintln!("{}", format!("Deleting dashboard {id}...").dimmed());
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = DashboardsApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!("Dashboard {id} deleted in profile '{profile}'.").green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }
    Ok(())
}

// ── Folders ───────────────────────────────────────────────────────────────────

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
        OutputFormat::Json => render::render_json(&all_rows)?,
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_rows).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                render::print_no_results("No dashboard folders found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, item)| {
                    vec![
                        profile.clone(),
                        item.id_str().unwrap_or("").to_string(),
                        item.name.clone().unwrap_or_default(),
                        item.parent_id_str().unwrap_or("").to_string(),
                    ]
                })
                .collect();
            render::render_table(&["ID", "Name", "Parent ID"], rows, include_profile);
        }
    }

    Ok(())
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
                    render::tag_get_result(&mut resp, &profile);
                }
                all_results.push((profile, created_id, resp));
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => {
            let vals: Vec<Value> = all_results.iter().map(|(_, _, v)| v.clone()).collect();
            render::render_json_auto(&vals)?;
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
                let rows: Vec<Vec<String>> = all_results
                    .iter()
                    .map(|(profile, id, _)| vec![profile.clone(), id.clone(), name.to_string()])
                    .collect();
                render::render_table(&["ID", "Name"], rows, true);
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

pub async fn run_folders_delete(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    eprintln!("{}", format!("Deleting dashboard folder {id}...").dimmed());
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = DashboardsApi::new(&t.client);
            api.folders_delete(&id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!("Folder {id} deleted in profile '{profile}'.").green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }
    Ok(())
}
