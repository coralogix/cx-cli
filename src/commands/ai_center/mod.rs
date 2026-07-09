//! `cx ai-center` — Coralogix AI Center (AI v3) configuration commands.
//!
//! Read/write access to the AI application inventory, configured evaluations/policies,
//! coverage, custom evaluations, and model pricing — the configuration that is *not*
//! in span telemetry. Telemetry (GenAI spans) is queried separately via `cx spans`.
//! See the `cx-ai-center` skill.

pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;
use api::AiCenterApi;

// ── Shared helpers ──────────────────────────────────────────────────

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading JSON definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading JSON definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

fn to_param_refs(params: &[(String, String)]) -> Vec<(&str, &str)> {
    params
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

/// Render a list of JSON rows + a text table across the three output formats.
fn emit_table(
    all_json: &[Value],
    rows: Vec<Vec<String>>,
    headers: &[&str],
    include_profile: bool,
    output: OutputFormat,
    empty_msg: &str,
) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json(all_json)?,
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if rows.is_empty() {
                render::print_no_results(empty_msg);
                return Ok(());
            }
            render::render_table(headers, rows, include_profile);
        }
    }
    Ok(())
}

/// Render one JSON object per profile (get / coverage / pricing / write results).
fn emit_objects(
    all: &[Value],
    include_profile: bool,
    output: OutputFormat,
    empty_msg: &str,
) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json_auto(all)?,
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            render::render_get_text(all, include_profile, empty_msg, None::<&dyn Fn(&Value)>)?;
        }
    }
    Ok(())
}

/// Read a string field off a raw item for a text-table column ("" if absent).
fn col<'a>(item: &'a Value, key: &str) -> &'a str {
    item.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Tag a raw item with its profile (multi-profile mode) and return it.
fn tag_item(mut item: Value, include_profile: bool, profile: &str) -> Value {
    if include_profile {
        if let Some(obj) = item.as_object_mut() {
            obj.insert("profile".into(), Value::String(profile.to_string()));
        }
    }
    item
}

/// Collect per-profile `Value` results, printing per-profile errors to stderr and
/// tagging each object with its profile in multi-profile mode.
fn collect_objects(per_profile: Vec<(String, Result<Value>)>, include_profile: bool) -> Vec<Value> {
    let mut all: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(mut val) => {
                if include_profile {
                    render::tag_get_result(&mut val, &profile);
                }
                all.push(val);
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }
    all
}

// ── Applications ────────────────────────────────────────────────────

pub async fn run_applications_list(
    targets: &[Arc<ExecutionTarget>],
    page_size: Option<u32>,
    page_offset: Option<u32>,
    evaluation_types: &[String],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching AI applications...".dimmed());
    let include_profile = targets.len() > 1;

    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(n) = page_size {
        params.push(("pageSize".into(), n.to_string()));
    }
    if let Some(n) = page_offset {
        params.push(("pageOffset".into(), n.to_string()));
    }
    for t in evaluation_types {
        params.push(("evaluationTypes".into(), t.clone()));
    }

    let per_profile = fan_out(targets, |t| {
        let params = params.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api.list_applications(&to_param_refs(&params)).await?)
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(items) => {
                for item in items {
                    rows.push(vec![
                        profile.clone(),
                        col(&item, "id").to_string(),
                        col(&item, "application").to_string(),
                        col(&item, "subsystem").to_string(),
                        render::bool_display(
                            item.get("guardrailsIntegrated").and_then(Value::as_bool),
                        ),
                    ]);
                    all_json.push(tag_item(item, include_profile, &profile));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    emit_table(
        &all_json,
        rows,
        &["ID", "Application", "Subsystem", "Guarded"],
        include_profile,
        output,
        "No AI applications found.",
    )
}

pub async fn run_applications_get(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching AI application {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api.get_application(&id).await?)
        }
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    emit_objects(&all, include_profile, output, "AI application not found.")
}

// ── Evaluations ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run_evaluations_list(
    targets: &[Arc<ExecutionTarget>],
    application: Option<&str>,
    subsystem: Option<&str>,
    evaluation_type: Option<&str>,
    page_size: Option<u32>,
    page_offset: Option<u32>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching AI evaluations...".dimmed());
    let include_profile = targets.len() > 1;

    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(v) = application {
        params.push(("application".into(), v.to_string()));
    }
    if let Some(v) = subsystem {
        params.push(("subsystem".into(), v.to_string()));
    }
    if let Some(v) = evaluation_type {
        params.push(("evaluationType".into(), v.to_string()));
    }
    if let Some(n) = page_size {
        params.push(("pageSize".into(), n.to_string()));
    }
    if let Some(n) = page_offset {
        params.push(("pageOffset".into(), n.to_string()));
    }

    let per_profile = fan_out(targets, |t| {
        let params = params.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api.list_evaluations(&to_param_refs(&params)).await?)
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(items) => {
                for item in items {
                    rows.push(vec![
                        profile.clone(),
                        col(&item, "id").to_string(),
                        col(&item, "application").to_string(),
                        col(&item, "subsystem").to_string(),
                        api::eval_config_type(&item),
                        col(&item, "target").to_string(),
                        render::bool_display(item.get("isEnabled").and_then(Value::as_bool)),
                        item.get("threshold")
                            .and_then(Value::as_f64)
                            .map(|t| t.to_string())
                            .unwrap_or_default(),
                    ]);
                    all_json.push(tag_item(item, include_profile, &profile));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    emit_table(
        &all_json,
        rows,
        &[
            "ID",
            "Application",
            "Subsystem",
            "Type",
            "Target",
            "Enabled",
            "Threshold",
        ],
        include_profile,
        output,
        "No AI evaluations found.",
    )
}

pub async fn run_evaluations_get(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching AI evaluation {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api.get_evaluation(&id).await?)
        }
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    emit_objects(&all, include_profile, output, "AI evaluation not found.")
}

pub async fn run_evaluations_create(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Creating AI evaluation...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api.create_evaluation(&body).await?)
        }
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    eprintln!("{}", "Created AI evaluation.".green());
    emit_objects(&all, include_profile, output, "No result.")
}

pub async fn run_evaluations_update(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", format!("Updating AI evaluation {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        let id = id.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api.update_evaluation(&id, &body).await?)
        }
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    eprintln!("{}", "Updated AI evaluation.".green());
    emit_objects(&all, include_profile, output, "No result.")
}

pub async fn run_evaluations_delete(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Deleting AI evaluation {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api.delete_evaluation(&id).await?)
        }
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    eprintln!("{}", "Deleted AI evaluation.".green());
    emit_objects(&all, include_profile, output, "No result.")
}

// ── Coverage (apps per evaluation type) ─────────────────────────────

pub async fn run_coverage(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching AI evaluation coverage...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = AiCenterApi::new(&t.client);
        Ok(api.count_apps_per_eval_type().await?)
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    emit_objects(&all, include_profile, output, "No coverage data.")
}

// ── Custom evaluations ──────────────────────────────────────────────

pub async fn run_custom_evaluations_list(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching custom evaluations...".dimmed());
    run_custom_evaluations_table(targets, None, output).await
}

pub async fn run_custom_evaluations_for_application(
    targets: &[Arc<ExecutionTarget>],
    application_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching custom evaluations for application {application_id}...").dimmed()
    );
    run_custom_evaluations_table(targets, Some(application_id.to_string()), output).await
}

/// Shared list/table path for both custom-evaluation list variants.
async fn run_custom_evaluations_table(
    targets: &[Arc<ExecutionTarget>],
    application_id: Option<String>,
    output: OutputFormat,
) -> Result<()> {
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let application_id = application_id.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            let resp = match application_id {
                Some(app_id) => api.list_custom_evaluations_for_application(&app_id).await?,
                None => api.list_custom_evaluations().await?,
            };
            Ok(resp)
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            // Items are raw API objects: the text table reads a few columns, but
            // JSON/agents output keeps the full policy (config, instructions, etc.).
            Ok(items) => {
                for item in items {
                    let app_count = item
                        .get("applicationIds")
                        .and_then(Value::as_array)
                        .map(|a| a.len())
                        .unwrap_or(0);
                    rows.push(vec![
                        profile.clone(),
                        col(&item, "id").to_string(),
                        col(&item, "name").to_string(),
                        app_count.to_string(),
                        col(&item, "description").chars().take(60).collect(),
                    ]);
                    all_json.push(tag_item(item, include_profile, &profile));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    emit_table(
        &all_json,
        rows,
        &["ID", "Name", "Apps", "Description"],
        include_profile,
        output,
        "No custom evaluations found.",
    )
}

pub async fn run_custom_evaluations_create(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Creating custom evaluation...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api.create_custom_evaluation(&body).await?)
        }
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    eprintln!("{}", "Created custom evaluation.".green());
    emit_objects(&all, include_profile, output, "No result.")
}

pub async fn run_custom_evaluations_update(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", format!("Updating custom evaluation {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        let id = id.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api.update_custom_evaluation(&id, &body).await?)
        }
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    eprintln!("{}", "Updated custom evaluation.".green());
    emit_objects(&all, include_profile, output, "No result.")
}

pub async fn run_add_policy(
    targets: &[Arc<ExecutionTarget>],
    evaluation_id: &str,
    application_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Attaching policy {evaluation_id} to application {application_id}...").dimmed()
    );
    let include_profile = targets.len() > 1;
    let evaluation_id = evaluation_id.to_string();
    let application_id = application_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let evaluation_id = evaluation_id.clone();
        let application_id = application_id.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api
                .add_policy_to_application(&evaluation_id, &application_id)
                .await?)
        }
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    eprintln!("{}", "Attached policy to application.".green());
    emit_objects(&all, include_profile, output, "No result.")
}

pub async fn run_remove_policy(
    targets: &[Arc<ExecutionTarget>],
    evaluation_id: &str,
    application_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Detaching policy {evaluation_id} from application {application_id}...").dimmed()
    );
    let include_profile = targets.len() > 1;
    let evaluation_id = evaluation_id.to_string();
    let application_id = application_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let evaluation_id = evaluation_id.clone();
        let application_id = application_id.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api
                .remove_policy_from_application(&evaluation_id, &application_id)
                .await?)
        }
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    eprintln!("{}", "Detached policy from application.".green());
    emit_objects(&all, include_profile, output, "No result.")
}

// ── Model pricing ───────────────────────────────────────────────────

pub async fn run_model_pricing_get(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching model pricing...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = AiCenterApi::new(&t.client);
        Ok(api.get_model_pricing().await?)
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    emit_objects(&all, include_profile, output, "No model pricing overrides.")
}

pub async fn run_model_pricing_set(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let prices = read_from_file(from_file)?;
    eprintln!("{}", "Setting model pricing...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let prices = prices.clone();
        async move {
            let api = AiCenterApi::new(&t.client);
            Ok(api.set_model_pricing(&prices).await?)
        }
    })
    .await;

    let all = collect_objects(per_profile, include_profile);
    eprintln!("{}", "Set model pricing.".green());
    emit_objects(&all, include_profile, output, "No result.")
}
