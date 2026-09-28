pub mod api;

use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{AlertSchedulerRule, AlertSchedulersApi};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn rule_to_json(rule: &AlertSchedulerRule, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "unique_identifier": rule.unique_identifier,
        "id": rule.id,
        "name": rule.name,
        "description": rule.description,
        "enabled": rule.enabled,
        "created_at": rule.created_at,
        "updated_at": rule.updated_at,
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

fn read_json_body(from_file: &str, entity_name: &str) -> Result<Value> {
    let raw = if from_file == "-" {
        eprintln!(
            "{}",
            format!("Reading {entity_name} definition from stdin...").dimmed()
        );
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading {entity_name} definition from {from_file}...").dimmed()
        );
        std::fs::read_to_string(from_file)?
    };

    let body: Value = serde_json::from_str(&raw)?;
    if !body.is_object() {
        bail!("{entity_name} JSON must be a JSON object");
    }
    Ok(body)
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching alert scheduler rules...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = AlertSchedulersApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, AlertSchedulerRule)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if !resp.alert_scheduler_rules.is_empty() {
            crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                crate::console_url::suppression_rules_url(b)
            })
            .await;
        }
        for rule in resp
            .alert_scheduler_rules
            .into_iter()
            .filter_map(|entry| entry.alert_scheduler_rule)
        {
            all_json.push(rule_to_json(&rule, include_profile, &profile));
            all_items.push((profile.clone(), rule));
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
                render::print_no_results("No alert scheduler rules found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, rule)| {
                    vec![
                        profile.clone(),
                        rule.unique_identifier.clone().unwrap_or_default(),
                        rule.name.clone().unwrap_or_default(),
                        render::bool_display(rule.enabled),
                        rule.created_at.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(&["ID", "Name", "Enabled", "Created"], rows, include_profile);
        }
    }

    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    rule_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching alert scheduler rule {rule_id}...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let id = rule_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move { Ok(AlertSchedulersApi::new(&t.client).get(&id).await?) }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, found) in report_errors_and_collect_successes(per_profile)? {
        let Some(mut val) = found else {
            continue;
        };
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::suppression_rule_url(b, rule_id)
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
            render::render_get_text(&all_results, include_profile, "Rule not found.", None)?;
        }
    }

    Ok(())
}

pub async fn run_create(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_json_body(from_file, "alert scheduler rule")?;

    eprintln!("{}", "Creating alert scheduler rule...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = AlertSchedulersApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(rule) = resp.alert_scheduler_rule {
            let name = rule.name.as_deref().unwrap_or("<unnamed>");
            let id = rule.unique_identifier.as_deref();
            render::print_created("Created", "rule", Some(name), id, &profile);
            if let Some(id) = id {
                crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                    crate::console_url::suppression_rule_url(b, id)
                })
                .await;
            }
            all_results.push(rule_to_json(&rule, include_profile, &profile));
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
    let body = read_json_body(from_file, "alert scheduler rule")?;

    eprintln!("{}", "Updating alert scheduler rule...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move { Ok(AlertSchedulersApi::new(&t.client).update(&body).await?) }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(rule) = resp.alert_scheduler_rule {
            let name = rule.name.as_deref().unwrap_or("<unnamed>");
            let id = rule.unique_identifier.as_deref();
            render::print_created("Updated", "rule", Some(name), id, &profile);
            if let Some(id) = id {
                crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                    crate::console_url::suppression_rule_url(b, id)
                })
                .await;
            }
            all_results.push(rule_to_json(&rule, include_profile, &profile));
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

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], rule_id: &str) -> Result<()> {
    eprintln!(
        "{}",
        format!("Deleting alert scheduler rule {rule_id}...").dimmed()
    );

    let id = rule_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = AlertSchedulersApi::new(&t.client);
            if api.delete(&id).await? {
                Ok(())
            } else {
                bail!(
                    "No suppression rule found with ID '{id}'. Run \
                     `cx alerts suppression-rules list` to find its uniqueIdentifier."
                )
            }
        }
    })
    .await;

    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Deleted rule {rule_id} in profile '{profile}'.").green()
        );
    }

    Ok(())
}
