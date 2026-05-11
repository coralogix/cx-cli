pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;
use api::{TeamGroup, TeamGroupsApi};

fn group_to_json(group: &TeamGroup, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "group_id": group.group_id,
        "name": group.display_name(),
        "description": group.display_description(),
        "team_id": group.team_id,
        "group_type": group.group_type,
        "created_at": group.created_at,
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
        eprintln!("{}", "Reading team group definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading team group definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching team groups...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = TeamGroupsApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, TeamGroup)> = Vec::new();
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
        OutputFormat::Yaml => render::render_yaml(&all_json)?,
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                render::print_no_results("No team groups found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, group)| {
                    vec![
                        profile.clone(),
                        group.group_id.map(|id| id.to_string()).unwrap_or_default(),
                        group.display_name().to_string(),
                        String::new(), // Members count not available in list response
                        group.display_description().to_string(),
                    ]
                })
                .collect();
            render::render_table(
                &["Group ID", "Name", "Members Count", "Description"],
                rows,
                include_profile,
            );
        }
    }
    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    group_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching team group {group_id}...").dimmed());
    let include_profile = targets.len() > 1;
    let group_id = group_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let group_id = group_id.clone();
        async move {
            let api = TeamGroupsApi::new(&t.client);
            Ok(api.get_by_id(&group_id).await?)
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, TeamGroup)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                if let Some(group) = resp.group {
                    all_json.push(group_to_json(&group, include_profile, &profile));
                    all_items.push((profile.clone(), group));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_json)?,
        OutputFormat::Yaml => render::render_yaml_auto(&all_json)?,
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                render::print_no_results("Team group not found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, group)| {
                    vec![
                        profile.clone(),
                        group.group_id.map(|id| id.to_string()).unwrap_or_default(),
                        group.display_name().to_string(),
                        group.display_description().to_string(),
                        group.group_type.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["Group ID", "Name", "Description", "Type"],
                rows,
                include_profile,
            );
        }
    }
    Ok(())
}

pub async fn run_get_by_name(
    targets: &[Arc<ExecutionTarget>],
    name: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching team group by name '{name}'...").dimmed()
    );
    let include_profile = targets.len() > 1;
    let name = name.to_string();

    let per_profile = fan_out(targets, |t| {
        let name = name.clone();
        async move {
            let api = TeamGroupsApi::new(&t.client);
            Ok(api.get_by_name(&name).await?)
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, TeamGroup)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                if let Some(group) = resp.group {
                    all_json.push(group_to_json(&group, include_profile, &profile));
                    all_items.push((profile.clone(), group));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_json)?,
        OutputFormat::Yaml => render::render_yaml_auto(&all_json)?,
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                render::print_no_results("Team group not found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, group)| {
                    vec![
                        profile.clone(),
                        group.group_id.map(|id| id.to_string()).unwrap_or_default(),
                        group.display_name().to_string(),
                        group.display_description().to_string(),
                        group.group_type.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["Group ID", "Name", "Description", "Type"],
                rows,
                include_profile,
            );
        }
    }
    Ok(())
}

pub async fn run_users(
    targets: &[Arc<ExecutionTarget>],
    group_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching users in team group {group_id}...").dimmed()
    );
    let include_profile = targets.len() > 1;
    let group_id = group_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let group_id = group_id.clone();
        async move {
            let api = TeamGroupsApi::new(&t.client);
            let resp = api.get_users(&group_id).await?;
            Ok(serde_json::to_value(resp.users)?)
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
        OutputFormat::Yaml => render::render_yaml_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            render::render_get_text(
                &all_results,
                include_profile,
                "No users found in group.",
                None::<&dyn Fn(&Value)>,
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
    eprintln!("{}", "Creating team group...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = TeamGroupsApi::new(&t.client);
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
                    let id = group
                        .group_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    eprintln!(
                        "{}",
                        format!("Created team group '{name}' (ID: {id}) in profile '{profile}'.")
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
        OutputFormat::Yaml => render::render_yaml_auto(&all_results)?,
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
    group_id: &str,
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", format!("Updating team group {group_id}...").dimmed());
    let group_id = group_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        let group_id = group_id.clone();
        async move {
            let api = TeamGroupsApi::new(&t.client);
            Ok(api.update(&group_id, &body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                if let Some(group) = resp.group {
                    eprintln!(
                        "{}",
                        format!("Updated team group in profile '{profile}'.").green()
                    );
                    all_results.push(group_to_json(&group, targets.len() > 1, &profile));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Yaml => render::render_yaml_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }
    Ok(())
}

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], group_id: &str) -> Result<()> {
    eprintln!("{}", format!("Deleting team group {group_id}...").dimmed());
    let group_id = group_id.to_string();
    let per_profile = fan_out(targets, |t| {
        let group_id = group_id.clone();
        async move {
            let api = TeamGroupsApi::new(&t.client);
            api.delete(&group_id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!("Team group {group_id} deleted in profile '{profile}'.").green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }
    Ok(())
}
