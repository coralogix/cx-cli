use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::api::recording_rules::{RecordingRuleGroup, RecordingRulesApi};
use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn group_to_json(
    group: &RecordingRuleGroup,
    include_profile: bool,
    profile: &str,
) -> Value {
    let mut v = json!({
        "id": group.id,
        "name": group.display_name(),
        "rules_count": group.rules_count(),
        "interval": group.interval,
        "created_at": group.created_at,
        "updated_at": group.updated_at,
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
        eprintln!(
            "{}",
            "Reading recording rule group definition from stdin...".dimmed()
        );
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading recording rule group definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching recording rule groups...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = RecordingRulesApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, RecordingRuleGroup)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for group in resp.groups {
                    all_json.push(group_to_json(&group, include_profile, &profile));
                    all_items.push((profile.clone(), group));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => render::render_json(&all_json)?,
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                render::print_no_results("No recording rule groups found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, group)| {
                    vec![
                        profile.clone(),
                        group.id.clone().unwrap_or_default(),
                        group.display_name().to_string(),
                        group.rules_count().to_string(),
                        group.interval.clone().unwrap_or_default(),
                        group.created_at.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Rules Count", "Interval", "Created"],
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
    eprintln!(
        "{}",
        format!("Fetching recording rule group {id}...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = RecordingRulesApi::new(&t.client);
            Ok(api.get(&id).await?)
        }
    })
    .await;

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
                "Recording rule group not found.",
                Some(&|val| {
                    if let Some(group_val) = val.get("group") {
                        if let Ok(group) =
                            serde_json::from_value::<RecordingRuleGroup>(group_val.clone())
                        {
                            println!("{}:       {}", "Name".bold(), group.display_name());
                            println!(
                                "{}:         {}",
                                "ID".bold(),
                                group.id.as_deref().unwrap_or("-")
                            );
                            println!(
                                "{}: {}",
                                "Rules Count".bold(),
                                group.rules_count()
                            );
                            println!(
                                "{}:   {}",
                                "Interval".bold(),
                                group.interval.as_deref().unwrap_or("-")
                            );
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

    eprintln!("{}", "Creating recording rule group...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = RecordingRulesApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                if let Some(group) = resp.group {
                    let name = group.display_name().to_string();
                    let id = group.id.as_deref().unwrap_or("unknown");
                    eprintln!(
                        "{}",
                        format!(
                            "Created recording rule group '{name}' (ID: {id}) in profile '{profile}'."
                        )
                        .green()
                    );
                    all_results.push(group_to_json(&group, include_profile, &profile));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
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
    id: &str,
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;

    eprintln!(
        "{}",
        format!("Updating recording rule group {id}...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        let id = id.clone();
        async move {
            let api = RecordingRulesApi::new(&t.client);
            Ok(api.update(&id, &body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                if let Some(group) = resp.group {
                    let name = group.display_name().to_string();
                    eprintln!(
                        "{}",
                        format!(
                            "Updated recording rule group '{name}' (ID: {id}) in profile '{profile}'."
                        )
                        .green()
                    );
                    all_results.push(group_to_json(&group, include_profile, &profile));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }

    Ok(())
}

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    eprintln!(
        "{}",
        format!("Deleting recording rule group {id}...").dimmed()
    );

    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = RecordingRulesApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;

    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!("Recording rule group {id} deleted in profile '{profile}'.").green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    Ok(())
}
