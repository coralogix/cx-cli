pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{RuleGroup, RuleGroupsApi};

fn rg_to_json(rg: &RuleGroup, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": rg.id,
        "name": rg.display_name(),
        "rules_count": rg.rules_count(),
        "enabled": rg.enabled,
        "order": rg.order,
        "creator": rg.creator,
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
        eprintln!("{}", "Reading rule group definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading rule group definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching rule groups...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = RuleGroupsApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;
    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, RuleGroup)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for rg in resp.rule_groups {
            all_json.push(rg_to_json(&rg, include_profile, &profile));
            all_items.push((profile.clone(), rg));
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
                render::print_no_results("No rule groups found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, rg)| {
                    vec![
                        profile.clone(),
                        rg.id.clone().unwrap_or_default(),
                        rg.display_name().to_string(),
                        rg.rules_count().to_string(),
                        render::bool_display(rg.enabled),
                        rg.order.map(|o| o.to_string()).unwrap_or_default(),
                        rg.creator.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Rules Count", "Enabled", "Order", "Creator"],
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
    eprintln!("{}", format!("Fetching rule group {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = RuleGroupsApi::new(&t.client);
            Ok(api.get(&id).await?)
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
                "Rule group not found.",
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
    eprintln!("{}", "Creating rule group...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = RuleGroupsApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(rg) = resp.rule_group {
            eprintln!(
                "{}",
                format!(
                    "Created rule group '{}' in profile '{profile}'.",
                    rg.display_name()
                )
                .green()
            );
            if let Some(id) = rg.id.as_deref() {
                if let Some(target) = crate::execution::find_target(targets, &profile) {
                    if let Some(base) = target.console_base().await {
                        render::print_console_link(&crate::console_url::parsing_rule_group_url(
                            &base, id,
                        ));
                    }
                }
            }
            all_results.push(rg_to_json(&rg, include_profile, &profile));
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
    eprintln!("{}", format!("Updating rule group {id}...").dimmed());
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        let id = id.clone();
        async move {
            let api = RuleGroupsApi::new(&t.client);
            Ok(api.update(&id, &body).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Updated rule group in profile '{profile}'.").green()
        );
        if let Some(target) = crate::execution::find_target(targets, &profile) {
            if let Some(base) = target.console_base().await {
                render::print_console_link(&crate::console_url::parsing_rule_group_url(&base, &id));
            }
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
        OutputFormat::Text => {}
    }
    Ok(())
}

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    eprintln!("{}", format!("Deleting rule group {id}...").dimmed());
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = RuleGroupsApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Rule group {id} deleted in profile '{profile}'.").green()
        );
    }
    Ok(())
}

pub async fn run_bulk_delete(targets: &[Arc<ExecutionTarget>], ids: &[String]) -> Result<()> {
    eprintln!("{}", "Bulk deleting rule groups...".dimmed());
    let body = json!({"ids": ids});
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = RuleGroupsApi::new(&t.client);
            api.bulk_delete(&body).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Rule groups deleted in profile '{profile}'.").green()
        );
    }
    Ok(())
}

pub async fn run_usage_limits(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching rule usage limits...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = RuleGroupsApi::new(&t.client);
        Ok(api.usage_limits().await?)
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
            for val in &all_results {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }
    Ok(())
}
