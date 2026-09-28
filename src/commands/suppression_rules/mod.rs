pub mod api;

use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::error::CxError;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{classify_rule_id, AlertSchedulerRule, AlertSchedulersApi, RuleIdKind};

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

/// Falls back to `id` so a body keyed by the version id can still be diagnosed.
fn update_body_identifier(body: &Value) -> Option<&str> {
    let rule = body.get("alertSchedulerRule").unwrap_or(body);
    rule.get("uniqueIdentifier")
        .and_then(|v| v.as_str())
        .or_else(|| rule.get("id").and_then(|v| v.as_str()))
}

fn warn_version_id_autocorrected(input: &str, unique_identifier: &str) {
    eprintln!(
        "{}",
        format!(
            "Note: '{input}' is a rule version id, not its stable id. Using uniqueIdentifier \
             '{unique_identifier}' instead - the version id changes on every update."
        )
        .yellow()
    );
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
        async move {
            let api = AlertSchedulersApi::new(&t.client);
            if let Some(val) = api.get(&id).await? {
                return Ok(Some((val, id)));
            }
            // A miss may be a version id; resolve it and retry.
            match classify_rule_id(&api, &id).await? {
                RuleIdKind::VersionId(uid) => {
                    warn_version_id_autocorrected(&id, &uid);
                    Ok(api.get(&uid).await?.map(|val| (val, uid)))
                }
                _ => Ok(None),
            }
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, found) in report_errors_and_collect_successes(per_profile)? {
        let Some((mut val, resolved_id)) = found else {
            continue;
        };
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::suppression_rule_url(b, &resolved_id)
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
    let identifier = update_body_identifier(&body).map(str::to_string);

    eprintln!("{}", "Updating alert scheduler rule...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        let identifier = identifier.clone();
        async move {
            let api = AlertSchedulersApi::new(&t.client);
            let err = match api.update(&body).await {
                Ok(resp) => return Ok(resp),
                Err(
                    e @ CxError::Api {
                        status: 400 | 404, ..
                    },
                ) => e,
                Err(e) => return Err(e.into()),
            };
            // On rejection, check whether the body named a version id.
            let Some(identifier) = identifier.as_deref() else {
                return Err(err.into());
            };
            match classify_rule_id(&api, identifier).await? {
                RuleIdKind::Addressable => Err(err.into()),
                RuleIdKind::VersionId(uid) => bail!(
                    "The update body identifies the rule by '{identifier}', which is a rule \
                     version id (not addressable). Use uniqueIdentifier '{uid}' instead."
                ),
                RuleIdKind::Unknown => bail!(
                    "No suppression rule found matching id '{identifier}'. Run \
                     `cx alerts suppression-rules list` to find its uniqueIdentifier. ({err})"
                ),
            }
        }
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
                return Ok(id);
            }
            // A miss may be a version id; resolve it and retry.
            let not_found = || {
                anyhow::anyhow!(
                    "No suppression rule found with ID '{id}'. This must be the rule's \
                     uniqueIdentifier, not its version id - run \
                     `cx alerts suppression-rules list` to find it."
                )
            };
            match classify_rule_id(&api, &id).await? {
                RuleIdKind::VersionId(uid) => {
                    warn_version_id_autocorrected(&id, &uid);
                    if api.delete(&uid).await? {
                        Ok(uid)
                    } else {
                        Err(not_found())
                    }
                }
                _ => Err(not_found()),
            }
        }
    })
    .await;

    for (profile, target) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Deleted rule {target} in profile '{profile}'.").green()
        );
    }

    Ok(())
}
