pub mod api;

use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{rule_found, AlertSchedulerRule, AlertSchedulersApi};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Both IDs are surfaced under the API's own names. `unique_identifier` is the
/// addressable one (get/update/delete/console link); `id` is the version id and
/// is included only because the API reports it. See `api::AlertSchedulerRule`.
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

/// Warn when an update body identifies the rule by `id` alone.
///
/// The backend keys updates off `uniqueIdentifier`; a body carrying only `id`
/// (the version id) is rejected with a bare `400 Bad Request: Invalid UUID
/// format` that names no field, which is near-impossible to diagnose from the
/// error alone. Advisory only - the request still goes out, since the check is
/// a heuristic over a body we otherwise pass through untouched.
fn warn_if_body_keyed_by_version_id(body: &Value) {
    let rule = body.get("alertSchedulerRule").unwrap_or(body);
    let has_unique = rule.get("uniqueIdentifier").is_some_and(|v| !v.is_null());
    let has_id = rule.get("id").is_some_and(|v| !v.is_null());
    if has_id && !has_unique {
        eprintln!(
            "{}",
            "Warning: the update body sets 'id' but not 'uniqueIdentifier'. 'id' is the rule \
             version id and is not addressable - the API rejects such bodies with a field-less \
             \"Invalid UUID format\" error. Use the rule's uniqueIdentifier (see \
             `cx alerts suppression-rules list`)."
                .yellow()
        );
    }
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
        // Each suppression rule has its own console route, so every row gets
        // its own consoleUrl rather than one shared page link. `console_base`
        // is cached per target and doesn't print, so resolving it once per
        // profile here is cheap.
        let console_base = match crate::execution::find_target(targets, &profile) {
            Some(target) => target.console_base().await,
            None => None,
        };
        // Also echo the suppression-rules page itself once per profile, the
        // way `alerts list` echoes the alerts page. Skipped when the profile
        // returned nothing, since there'd be nothing to look at.
        if !resp.alert_scheduler_rules.is_empty() {
            crate::execution::console_link_for_profile(targets, &profile, |b| {
                crate::console_url::suppression_rules_url(b)
            })
            .await;
        }
        // List items are wrapped: {"alertSchedulerRule": {...},
        // "nextActiveTimeframes": [...]}. Unwrap to the rule itself.
        for rule in resp
            .alert_scheduler_rules
            .into_iter()
            .filter_map(|entry| entry.alert_scheduler_rule)
        {
            let mut json = rule_to_json(&rule, include_profile, &profile);
            if let (Some(base), Some(id)) = (&console_base, rule.unique_identifier.as_deref()) {
                render::tag_console_url(
                    &mut json,
                    &crate::console_url::suppression_rule_url(base, id),
                );
            }
            all_json.push(json);
            all_items.push((profile.clone(), rule));
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
                render::print_no_results("No alert scheduler rules found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, rule)| {
                    vec![
                        profile.clone(),
                        // The addressable id - the version id has no use to a
                        // human reading a table, so it's left to json/agents.
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
            Ok(api.get(&id).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        // An unknown id answers 200 `{}` instead of 404, so drop misses here
        // and let the "Rule not found." path handle them.
        if !rule_found(&val) {
            continue;
        }
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        // `rule_id` is the rule's unique_identifier - the fetch only succeeded
        // because it was - so it's what the console link needs.
        crate::execution::tag_console_link_for_profile(targets, &profile, &mut val, |b| {
            crate::console_url::suppression_rule_url(b, rule_id)
        })
        .await;
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
            // Report the addressable id, not the version id - this is the
            // value the user feeds back into get/update/delete.
            let id = rule.unique_identifier.as_deref();
            render::print_created("Created", "rule", Some(name), id, &profile);
            let mut rule_json = rule_to_json(&rule, include_profile, &profile);
            if let Some(id) = id {
                crate::execution::tag_console_link_for_profile(
                    targets,
                    &profile,
                    &mut rule_json,
                    |b| crate::console_url::suppression_rule_url(b, id),
                )
                .await;
            }
            all_results.push(rule_json);
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
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_json_body(from_file, "alert scheduler rule")?;
    warn_if_body_keyed_by_version_id(&body);

    eprintln!("{}", "Updating alert scheduler rule...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = AlertSchedulersApi::new(&t.client);
            Ok(api.update(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(rule) = resp.alert_scheduler_rule {
            let name = rule.name.as_deref().unwrap_or("<unnamed>");
            let id = rule.unique_identifier.as_deref();
            render::print_created("Updated", "rule", Some(name), id, &profile);
            let mut rule_json = rule_to_json(&rule, include_profile, &profile);
            if let Some(id) = id {
                crate::execution::tag_console_link_for_profile(
                    targets,
                    &profile,
                    &mut rule_json,
                    |b| crate::console_url::suppression_rule_url(b, id),
                )
                .await;
            }
            all_results.push(rule_json);
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
            // DELETE answers 200 for an unknown id without deleting anything,
            // so without this check deleting by the wrong id (the rule's
            // version id is the easy mistake) reports success while the rule
            // stays put. Confirm it resolves before claiming we removed it.
            if !rule_found(&api.get(&id).await?) {
                bail!(
                    "No suppression rule found with ID '{id}'. This must be the rule's \
                     uniqueIdentifier, not its version id - run \
                     `cx alerts suppression-rules list` to find it."
                );
            }
            api.delete(&id).await?;
            Ok(())
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
