pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{E2mApi, E2mDefinition};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn e2m_to_json(def: &E2mDefinition, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": def.id,
        "name": def.display_name(),
        "type": def.display_type(),
        "metric_name": def.metric_name,
        "is_active": def.is_active,
        "create_time": def.create_time,
        "update_time": def.update_time,
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading E2M definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading E2M definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching E2M definitions...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = E2mApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, E2mDefinition)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        // Print the E2M definitions list page link to stderr once per
        // profile. Skip when there are no definitions, since there's
        // nothing to view.
        if !resp.e2m.is_empty() {
            crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                crate::console_url::e2m_definitions_url(b)
            })
            .await;
        }
        for def in resp.e2m {
            all_json.push(e2m_to_json(&def, include_profile, &profile));
            all_items.push((profile.clone(), def));
        }
    }

    match output {
        OutputFormat::Json => render::render_json(&all_json)?,
        OutputFormat::Toon => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                render::print_no_results("No E2M definitions found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, def)| {
                    vec![
                        profile.clone(),
                        def.id.clone().unwrap_or_default(),
                        def.display_name().to_string(),
                        def.display_type(),
                        def.metric_name.clone().unwrap_or_default(),
                        def.create_time.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Type", "Metric Name", "Created"],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching E2M definition {id}...").dimmed());

    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = E2mApi::new(&t.client);
            Ok(api.get(&id).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::e2m_url(b, &id)
        })
        .await;
        all_results.push(val);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            render::render_get_text(
                &all_results,
                include_profile,
                "E2M definition not found.",
                Some(&|val| {
                    if let Some(e2m_val) = val.get("e2m") {
                        if let Ok(def) = serde_json::from_value::<E2mDefinition>(e2m_val.clone()) {
                            println!("{}:        {}", "Name".bold(), def.display_name());
                            println!(
                                "{}:          {}",
                                "ID".bold(),
                                def.id.as_deref().unwrap_or("-")
                            );
                            println!("{}:        {}", "Type".bold(), def.display_type());
                            println!(
                                "{}: {}",
                                "Metric Name".bold(),
                                def.metric_name.as_deref().unwrap_or("-")
                            );
                            if let Some(ref desc) = def.description {
                                if !desc.is_empty() {
                                    println!("{}: {}", "Description".bold(), desc);
                                }
                            }
                        }
                    }
                }),
            )?;
        }
    }

    Ok(())
}

pub async fn run_create(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;

    eprintln!("{}", "Creating E2M definition...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = E2mApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(def) = resp.e2m {
            let name = def.display_name().to_string();
            render::print_created("Created", "E2M", Some(&name), def.id.as_deref(), &profile);
            if let Some(id) = def.id.as_deref() {
                crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                    crate::console_url::e2m_url(b, id)
                })
                .await;
            }
            let val = e2m_to_json(&def, include_profile, &profile);
            all_results.push(val);
        }
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }

    Ok(())
}

pub async fn run_update(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;

    eprintln!("{}", "Updating E2M definition...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = E2mApi::new(&t.client);
            Ok(api.replace(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(def) = resp.e2m {
            let name = def.display_name().to_string();
            render::print_created("Updated", "E2M", Some(&name), def.id.as_deref(), &profile);
            if let Some(id) = def.id.as_deref() {
                crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                    crate::console_url::e2m_url(b, id)
                })
                .await;
            }
            let val = e2m_to_json(&def, include_profile, &profile);
            all_results.push(val);
        }
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }

    Ok(())
}

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    eprintln!("{}", format!("Deleting E2M definition {id}...").dimmed());

    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = E2mApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;

    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("E2M {id} deleted in profile '{profile}'.").green()
        );
    }

    Ok(())
}

pub async fn run_labels_cardinality(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching E2M labels cardinality...".dimmed());

    let per_profile = fan_out(targets, |t| async move {
        let api = E2mApi::new(&t.client);
        Ok(api.labels_cardinality().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    for (_profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for label in resp.labels {
            all_json.push(label);
        }
    }

    match output {
        OutputFormat::Json => render::render_json(&all_json)?,
        OutputFormat::Toon => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_json.is_empty() {
                render::print_no_results("No E2M labels cardinality data found.");
                return Ok(());
            }
            for val in &all_json {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}

pub async fn run_limits(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching E2M limits...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = E2mApi::new(&t.client);
        Ok(api.limits().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        let mut v = json!({
            "limit": resp.limit,
            "used": resp.used,
        });
        if include_profile {
            if let Value::Object(ref mut m) = v {
                m.insert("profile".to_string(), Value::String(profile.clone()));
            }
        }
        all_json.push(v);
    }

    match output {
        OutputFormat::Json => render::render_json(&all_json)?,
        OutputFormat::Toon => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_json.is_empty() {
                render::print_no_results("No E2M limits data found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_json
                .iter()
                .map(|v| {
                    vec![
                        v.get("profile")
                            .and_then(|p| p.as_str())
                            .unwrap_or("")
                            .to_string(),
                        v.get("limit")
                            .and_then(|l| l.as_u64())
                            .map(|l| l.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        v.get("used")
                            .and_then(|u| u.as_u64())
                            .map(|u| u.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                    ]
                })
                .collect();
            render::render_table(&["Limit", "Used"], rows, include_profile);
        }
    }

    Ok(())
}
