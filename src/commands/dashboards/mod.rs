use std::io::Read;
use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use rand::RngExt;
use serde::Serialize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

pub mod api;

use crate::config::OutputFormat;
use crate::execution::{report_errors_and_collect_successes, fan_out, ExecutionTarget};
use crate::render;
use api::{
    DashboardFolderItem, DashboardSearchResult, DashboardsApi, IssueSeverity, QueryByFieldResult,
    QuerySearchResult,
};

use crate::safety::confirm_destructive;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// JSON key for the source profile when merging multi-profile dashboard REST rows.
const JSON_KEY_PROFILE: &str = "profile";

/// Builds one catalog row as JSON for `json` / `agents` output after fan-out.
///
/// When `include_profile` is true (multiple `--profile`), injects the profile key so merged
/// arrays stay attributable per account; text mode uses a separate table path.
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
            m.insert(
                JSON_KEY_PROFILE.to_string(),
                Value::String(profile.to_string()),
            );
        }
    }
    v
}

/// Builds one dashboard-folder row as JSON for `json` / `agents` output after fan-out.
///
/// Same contract as `catalog_item_to_json`: folder list responses are merged across
/// profiles, so we normalize each item to a plain object (string ids via `id_str` /
/// `parent_id_str`) and optionally add the profile key when rendering multi-profile results.
fn folder_item_to_json(item: &DashboardFolderItem, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": item.id_str(),
        "name": item.name,
        "parent_id": item.parent_id_str(),
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert(
                JSON_KEY_PROFILE.to_string(),
                Value::String(profile.to_string()),
            );
        }
    }
    v
}

/// One merged row for `json` / `agents`: `serde_json::to_value` (field names = JSON keys), then optional `profile`.
pub fn profiled_api_row_to_json<T: Serialize>(
    profile: &str,
    row: &T,
    include_profile: bool,
    row_kind: &str,
) -> Result<Value> {
    let mut v = serde_json::to_value(row)
        .map_err(|e| anyhow::anyhow!("failed to serialize {row_kind} row: {e}"))?;
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert(
                JSON_KEY_PROFILE.to_string(),
                Value::String(profile.to_string()),
            );
        }
    }
    Ok(v)
}

/// Runs `semantic_search` on every target **concurrently**, flattens `(profile, row)` pairs,
/// and errors if **all** profiles fail (so CI/scripts see a non-zero exit when nothing succeeded).
///
/// `fan_out` schedules one async task per profile. Each task clones the query string, builds a
/// [`DashboardsApi`] for that profile's HTTP client, and awaits the same REST call; answers are
/// stitched together so `-p a -p b` produces one combined list.
async fn collect_semantic_search_results(
    targets: &[Arc<ExecutionTarget>],
    query_text: &str,
    limit: u32,
) -> Result<Vec<(String, DashboardSearchResult)>> {
    let query_owned = query_text.to_string();

    let per_profile = fan_out(targets, |target| {
        let query_clone = query_owned.clone();
        async move {
            let api = DashboardsApi::new(&target.client);
            Ok(api.semantic_search(&query_clone, limit).await?)
        }
    })
    .await;

    let mut all_results: Vec<(String, DashboardSearchResult)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for r in resp.results {
            all_results.push((profile.clone(), r));
        }
    }
    Ok(all_results)
}

fn semantic_search_merged_json_rows(
    rows: &[(String, DashboardSearchResult)],
    include_profile: bool,
) -> Result<Vec<Value>> {
    rows.iter()
        .map(|(profile, r)| {
            profiled_api_row_to_json(profile, r, include_profile, "dashboard semantic search")
        })
        .collect()
}

fn render_semantic_search_text_table(
    rows: &[(String, DashboardSearchResult)],
    include_profile: bool,
) {
    if rows.is_empty() {
        println!("{}", "No matching dashboards found.".yellow());
        return;
    }
    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|(profile, r)| {
            vec![
                profile.clone(),
                r.dashboard_name.clone().unwrap_or_default(),
                r.dashboard_folder.clone().unwrap_or_default(),
                r.widget_count.map(|n| n.to_string()).unwrap_or_default(),
                format!("{:.3}", r.similarity),
                r.semantic_description
                    .as_deref()
                    .or(r.description.as_deref())
                    .unwrap_or_default()
                    .chars()
                    .take(60)
                    .collect(),
            ]
        })
        .collect();
    render::render_table(
        &["Name", "Folder", "Widgets", "Similarity", "Description"],
        table_rows,
        include_profile,
    );
}

async fn collect_query_search_results(
    targets: &[Arc<ExecutionTarget>],
    query_text: &str,
    limit: u32,
) -> Result<Vec<(String, QuerySearchResult)>> {
    let query_owned = query_text.to_string();
    let per_profile = fan_out(targets, |target| {
        let query_clone = query_owned.clone();
        async move {
            let api = DashboardsApi::new(&target.client);
            Ok(api.search_queries(&query_clone, limit).await?)
        }
    })
    .await;

    let mut all_results: Vec<(String, QuerySearchResult)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for r in resp.results {
            all_results.push((profile.clone(), r));
        }
    }
    Ok(all_results)
}

fn query_search_merged_json_rows(
    rows: &[(String, QuerySearchResult)],
    include_profile: bool,
) -> Result<Vec<Value>> {
    rows.iter()
        .map(|(profile, r)| {
            profiled_api_row_to_json(profile, r, include_profile, "dashboard query search")
        })
        .collect()
}

fn render_query_search_text_table(rows: &[(String, QuerySearchResult)], include_profile: bool) {
    if rows.is_empty() {
        println!("{}", "No matching queries found.".yellow());
        return;
    }
    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|(profile, r)| {
            vec![
                profile.clone(),
                r.query_text.clone(),
                r.dashboard_name.clone().unwrap_or_default(),
                r.widget_title.clone().unwrap_or_default(),
                format!("{:.3}", r.similarity),
            ]
        })
        .collect();
    render::render_table(
        &["Query", "Dashboard", "Widget", "Similarity"],
        table_rows,
        include_profile,
    );
}

async fn collect_queries_by_field_results(
    targets: &[Arc<ExecutionTarget>],
    field_path: &str,
    limit: u32,
) -> Result<Vec<(String, QueryByFieldResult)>> {
    let field_path_owned = field_path.to_string();
    let per_profile = fan_out(targets, |target| {
        let field_path_clone = field_path_owned.clone();
        async move {
            let api = DashboardsApi::new(&target.client);
            Ok(api.queries_by_field(&field_path_clone, limit).await?)
        }
    })
    .await;

    let mut all_results: Vec<(String, QueryByFieldResult)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for r in resp.queries {
            all_results.push((profile.clone(), r));
        }
    }
    Ok(all_results)
}

fn queries_by_field_merged_json_rows(
    rows: &[(String, QueryByFieldResult)],
    include_profile: bool,
) -> Result<Vec<Value>> {
    rows.iter()
        .map(|(profile, r)| {
            profiled_api_row_to_json(profile, r, include_profile, "dashboard queries-by-field")
        })
        .collect()
}

fn render_queries_by_field_text_table(
    rows: &[(String, QueryByFieldResult)],
    field_path: &str,
    include_profile: bool,
) {
    if rows.is_empty() {
        println!(
            "{}",
            format!("No queries found referencing {field_path:?}.").yellow()
        );
        return;
    }
    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|(profile, r)| {
            vec![
                profile.clone(),
                r.query_text.clone(),
                r.dashboard_name.clone().unwrap_or_default(),
                r.widget_title.clone().unwrap_or_default(),
                r.matched_fields.join(", "),
            ]
        })
        .collect();
    render::render_table(
        &["Query", "Dashboard", "Widget", "Matched Fields"],
        table_rows,
        include_profile,
    );
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_semantic_search(
    targets: &[Arc<ExecutionTarget>],
    query_text: &str,
    limit: u32,
    output: OutputFormat,
) -> Result<()> {
    if query_text.trim().is_empty() {
        bail!("query text cannot be empty");
    }
    eprintln!(
        "{}",
        format!("Searching dashboards for: {query_text:?}…").dimmed()
    );

    let include_profile = targets.len() > 1;
    let all_results = collect_semantic_search_results(targets, query_text, limit).await?;

    match output {
        OutputFormat::Json => {
            let json_rows = semantic_search_merged_json_rows(&all_results, include_profile)?;
            render::render_json(&json_rows)?;
        }
        OutputFormat::Agents => {
            let json_rows = semantic_search_merged_json_rows(&all_results, include_profile)?;
            render::render_agents(&json_rows)?;
        }
        OutputFormat::Text => render_semantic_search_text_table(&all_results, include_profile),
    }

    Ok(())
}

pub async fn run_catalog(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching dashboard catalog...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = DashboardsApi::new(&t.client);
        Ok(api.catalog().await?)
    })
    .await;

    let mut all_rows: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, api::DashboardCatalogItem)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for item in resp.items {
            all_rows.push(catalog_item_to_json(&item, include_profile, &profile));
            all_items.push((profile.clone(), item));
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

pub async fn run_search(
    targets: &[Arc<ExecutionTarget>],
    query_text: &str,
    limit: u32,
    output: OutputFormat,
) -> Result<()> {
    if query_text.trim().is_empty() {
        bail!("query text cannot be empty");
    }
    eprintln!(
        "{}",
        format!("Searching dashboard queries for: {query_text:?}…").dimmed()
    );

    let include_profile = targets.len() > 1;
    let all_results = collect_query_search_results(targets, query_text, limit).await?;

    match output {
        OutputFormat::Json => {
            let json_rows = query_search_merged_json_rows(&all_results, include_profile)?;
            render::render_json(&json_rows)?;
        }
        OutputFormat::Agents => {
            let json_rows = query_search_merged_json_rows(&all_results, include_profile)?;
            render::render_agents(&json_rows)?;
        }
        OutputFormat::Text => render_query_search_text_table(&all_results, include_profile),
    }

    Ok(())
}

pub async fn run_queries_by_field(
    targets: &[Arc<ExecutionTarget>],
    field_path: &str,
    limit: u32,
    output: OutputFormat,
) -> Result<()> {
    if field_path.trim().is_empty() {
        bail!("field path cannot be empty");
    }
    eprintln!(
        "{}",
        format!("Finding queries referencing field {field_path:?}…").dimmed()
    );

    let include_profile = targets.len() > 1;
    let all_results = collect_queries_by_field_results(targets, field_path, limit).await?;

    match output {
        OutputFormat::Json => {
            let json_rows = queries_by_field_merged_json_rows(&all_results, include_profile)?;
            render::render_json(&json_rows)?;
        }
        OutputFormat::Agents => {
            let json_rows = queries_by_field_merged_json_rows(&all_results, include_profile)?;
            render::render_agents(&json_rows)?;
        }
        OutputFormat::Text => {
            render_queries_by_field_text_table(&all_results, field_path, include_profile);
        }
    }

    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    dashboard_id: &str,
    output: OutputFormat,
) -> Result<()> {
    if dashboard_id.trim().is_empty() {
        bail!("dashboard id cannot be empty");
    }
    eprintln!(
        "{}",
        format!("Fetching dashboard {dashboard_id}...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let dashboard_id_owned = dashboard_id.to_string();

    let per_profile = fan_out(targets, |target| {
        let dashboard_id_clone = dashboard_id_owned.clone();
        async move {
            let api = DashboardsApi::new(&target.client);
            Ok(api.get(&dashboard_id_clone).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        all_results.push(val);
    }

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
    for (profile, mut resp) in report_errors_and_collect_successes(per_profile)? {
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
    yes: bool,
    agent_mode: bool,
) -> Result<()> {
    let dashboard = read_dashboard_body(from_file)?;

    let dash_id = dashboard
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Dashboard JSON is missing required 'id' field. \
                 Use `cx dashboards get <id> -o json` to fetch the full definition, \
                 then pass the edited JSON to `cx dashboards replace`."
            )
        })?;

    let name = dashboard
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("<unnamed>")
        .to_string();

    confirm_destructive(
        &format!("Replace dashboard '{name}' ({dash_id})?"),
        yes,
        agent_mode,
    )?;

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
    let dashboard_id_owned = id.to_string();
    let per_profile = fan_out(targets, |target| {
        let dashboard_id_clone = dashboard_id_owned.clone();
        async move {
            let api = DashboardsApi::new(&target.client);
            api.delete(&dashboard_id_clone).await?;
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
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for item in resp.folders {
            all_rows.push(folder_item_to_json(&item, include_profile, &profile));
            all_items.push((profile.clone(), item));
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
    if name.trim().is_empty() {
        bail!("folder name cannot be empty");
    }
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
    for (profile, mut resp) in report_errors_and_collect_successes(per_profile)? {
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
    let folder_id_owned = id.to_string();
    let per_profile = fan_out(targets, |target| {
        let folder_id_clone = folder_id_owned.clone();
        async move {
            let api = DashboardsApi::new(&target.client);
            api.folders_delete(&folder_id_clone).await?;
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

// ── Check (CheckDashboard) ────────────────────────────────────────────────────

/// Build the `CheckDashboardRequest` JSON body for the `dashboardId` oneof arm.
fn check_body_from_id(dashboard_id: &str) -> Value {
    json!({ "dashboardId": dashboard_id })
}

/// Build the `CheckDashboardRequest` JSON body for the `dashboard` oneof arm,
/// reusing `read_dashboard_body` to normalize a bare doc or a
/// `{ "dashboard": {...} }` wrapper into the inner dashboard object.
fn check_body_from_file(from_file: &str) -> Result<Value> {
    let dashboard = read_dashboard_body(from_file)?;
    Ok(json!({ "dashboard": dashboard }))
}

/// Colorize a severity string for text output.
fn severity_colored(severity: IssueSeverity) -> String {
    let s = match severity {
        IssueSeverity::SeverityError => "SEVERITY_ERROR",
        IssueSeverity::SeverityWarning => "SEVERITY_WARNING",
        IssueSeverity::SeverityUnspecified => "SEVERITY_UNSPECIFIED",
    };
    match severity {
        IssueSeverity::SeverityError => s.red().to_string(),
        IssueSeverity::SeverityWarning => s.yellow().to_string(),
        IssueSeverity::SeverityUnspecified => s.dimmed().to_string(),
    }
}

/// Build one issue row as JSON for `json` / `agents` output.
fn issue_json_row(issue: &api::DashboardCheckIssue, profile: &str, include_profile: bool) -> Value {
    let mut row = serde_json::to_value(issue).unwrap_or_else(|_| json!({}));
    if include_profile {
        if let Value::Object(ref mut m) = row {
            m.insert(
                JSON_KEY_PROFILE.to_string(),
                Value::String(profile.to_string()),
            );
        }
    }
    row
}

/// Validate a dashboard definition without persisting it
/// (`DashboardsService.CheckDashboard`).
///
/// Exactly one of `from_file` or `dashboard_id` must be supplied:
/// - `from_file` — path to a JSON file (or `-` for stdin) holding either a
///   bare dashboard doc or a `{ "dashboard": {...} }` wrapper.
/// - `dashboard_id` — validate a stored dashboard by id.
///
/// Read-only. Exits non-zero if any error-severity issue is found (CI gate).
/// In multi-profile fan-out, any profile returning error-severity issues
/// causes a non-zero exit, even if other profiles are clean — this is a
/// deliberate carve-out from the usual "exit 0 if any profile succeeds" rule,
/// because `check` is a validation gate first.
pub async fn run_check(
    targets: &[Arc<ExecutionTarget>],
    from_file: Option<&str>,
    dashboard_id: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    // Mutually exclusive sources. clap enforces "both" via `conflicts_with`;
    // we guard "neither" here with a clear message.
    let body = match (from_file, dashboard_id) {
        (Some(path), None) => {
            eprintln!("{}", "Checking dashboard from file...".dimmed());
            check_body_from_file(path)?
        }
        (None, Some(id)) => {
            if id.trim().is_empty() {
                bail!("dashboard id cannot be empty");
            }
            eprintln!("{}", format!("Checking dashboard {id}...").dimmed());
            check_body_from_id(id)
        }
        (Some(_), Some(_)) => {
            bail!("`--from-file` and `<dashboard_id>` are mutually exclusive; specify one")
        }
        (None, None) => {
            bail!("specify either `--from-file <path>` or a `<dashboard_id>` to check")
        }
    };

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = DashboardsApi::new(&t.client);
            Ok(api.check(&body).await?)
        }
    })
    .await;

    let mut all_issues: Vec<(String, Vec<api::DashboardCheckIssue>)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        all_issues.push((profile, resp.issues));
    }

    // CI-gate semantics: any error-severity issue (in any profile) fails.
    let total_errors = all_issues
        .iter()
        .flat_map(|(_, issues)| issues.iter())
        .filter(|i| i.severity.is_failure())
        .count();
    let total_issues = all_issues.iter().map(|(_, i)| i.len()).sum::<usize>();

    // Render.
    match output {
        OutputFormat::Json => {
            let mut rows: Vec<Value> = Vec::new();
            for (profile, issues) in &all_issues {
                for issue in issues {
                    rows.push(issue_json_row(issue, profile, include_profile));
                }
            }
            render::render_json(&rows)?;
        }
        OutputFormat::Agents => {
            let mut rows: Vec<Value> = Vec::new();
            for (profile, issues) in &all_issues {
                for issue in issues {
                    rows.push(issue_json_row(issue, profile, include_profile));
                }
            }
            render::render_agents(&rows)?;
        }
        OutputFormat::Text => {
            if total_issues == 0 {
                println!("{}", "Dashboard is valid (no issues)".green());
            } else {
                let mut rows: Vec<Vec<String>> = Vec::new();
                for (profile, issues) in &all_issues {
                    for issue in issues {
                        let row = vec![
                            profile.clone(),
                            severity_colored(issue.severity),
                            issue.location.clone().unwrap_or_default(),
                            issue.message.clone().unwrap_or_default(),
                        ];
                        rows.push(row);
                    }
                }
                render::render_table(&["Severity", "Location", "Message"], rows, include_profile);
            }
        }
    }

    if total_errors > 0 {
        bail!(
            "dashboard check found {total_errors} error(s) across profile(s); {} warning(s) ignored",
            total_issues - total_errors
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard for the `render_table` profile-column contract.
    ///
    /// `format_table` strips the first cell of every row when
    /// `include_profile` is false (single-profile mode). Callers must
    /// therefore push the profile as the first element unconditionally;
    /// conditional placement causes the severity column to be dropped and
    /// the remaining columns to shift left. This test constructs rows the
    /// same way `run_check` does and asserts the severity text survives
    /// rendering in single-profile mode.
    #[test]
    fn run_check_rows_preserve_severity_in_single_profile_output() {
        let profile = "mock-profile";
        let issue = api::DashboardCheckIssue {
            severity: api::IssueSeverity::SeverityWarning,
            message: Some("Query uses deprecated function 'timeShift'".to_string()),
            location: Some("/sections/1/rows/0/widgets/0/queries/0".to_string()),
        };

        // Mirrors the row construction in `run_check`'s Text branch.
        let row = vec![
            profile.to_string(),
            severity_colored(issue.severity),
            issue.location.clone().unwrap_or_default(),
            issue.message.clone().unwrap_or_default(),
        ];

        let rendered = render::format_table(
            &["Severity", "Location", "Message"],
            vec![row],
            false, // single-profile mode
        );

        // Severity text must be present (the bug dropped it via skip(1)).
        assert!(
            rendered.contains("SEVERITY_WARNING"),
            "severity column dropped in single-profile output: {rendered}"
        );
        assert!(
            rendered.contains("/sections/1/rows/0/widgets/0/queries/0"),
            "location column missing in single-profile output: {rendered}"
        );
        assert!(
            rendered.contains("Query uses deprecated function 'timeShift'"),
            "message column missing in single-profile output: {rendered}"
        );
    }
}
