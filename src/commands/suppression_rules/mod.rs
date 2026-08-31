pub mod api;

use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{classify_rule_id, rule_found, AlertSchedulerRule, AlertSchedulersApi, RuleIdKind};

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

/// The identifier an update body uses to name its rule, if any.
///
/// Accepts either a top-level body or one nested under `alertSchedulerRule`,
/// preferring `uniqueIdentifier` (the addressable id) and falling back to `id`
/// (the version id) so a body keyed by the wrong field is still classified
/// rather than sent blindly.
fn update_body_identifier(body: &Value) -> Option<&str> {
    let rule = body.get("alertSchedulerRule").unwrap_or(body);
    rule.get("uniqueIdentifier")
        .and_then(|v| v.as_str())
        .or_else(|| rule.get("id").and_then(|v| v.as_str()))
}

/// Warn that an input id was a rule version id and we auto-corrected it.
///
/// `get`/`delete` take a rule id but silently accept the version id (the API
/// answers 200 either way), so when [`classify_rule_id`] resolves the version
/// id to its stable `unique_identifier` we tell the user rather than switching
/// ids behind their back.
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
        // Echo the suppression-rules page once per profile, the way
        // `alerts list` echoes the alerts page. Skipped when the profile
        // returned nothing, since there'd be nothing to look at.
        if !resp.alert_scheduler_rules.is_empty() {
            crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
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
            let val = api.get(&id).await?;
            if rule_found(&val) {
                return Ok((val, id));
            }
            // A miss might just be a version id. One extra `list` call tells us,
            // and if so we re-fetch by the addressable id and carry it back so
            // the console link points at the rule the user actually got.
            match classify_rule_id(&api, &id).await? {
                RuleIdKind::VersionId(uid) => {
                    warn_version_id_autocorrected(&id, &uid);
                    let val = api.get(&uid).await?;
                    Ok((val, uid))
                }
                // Not a version id - hand back the empty body for the
                // "Rule not found." path. (`Addressable` is unreachable after a
                // `get` miss, but re-using `id` is the correct no-op if it ever
                // arises from a transient miss.)
                _ => Ok((val, id)),
            }
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, (mut val, resolved_id)) in report_errors_and_collect_successes(per_profile)? {
        // An unknown id answers 200 `{}` instead of 404, so drop misses here
        // and let the "Rule not found." path handle them.
        if !rule_found(&val) {
            continue;
        }
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        // `resolved_id` is the rule's unique_identifier - the fetch only
        // succeeded because it was - so it's what the console link needs.
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
            // Report the addressable id, not the version id - this is the
            // value the user feeds back into get/update/delete.
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
            // The backend keys updates off `uniqueIdentifier` and rejects a
            // version id with a field-less "400 Invalid UUID format". Catch that
            // before sending: one `list` call tells us which id the body carries,
            // and we point the user at the addressable one rather than the PUT
            // going out to fail cryptically.
            if let Some(identifier) = identifier.as_deref() {
                match classify_rule_id(&api, identifier).await? {
                    RuleIdKind::Addressable => {}
                    RuleIdKind::VersionId(uid) => bail!(
                        "The update body identifies the rule by '{identifier}', which is a rule \
                         version id (not addressable). Use uniqueIdentifier '{uid}' instead."
                    ),
                    RuleIdKind::Unknown => bail!(
                        "No suppression rule found matching id '{identifier}'. Run \
                         `cx alerts suppression-rules list` to find its uniqueIdentifier."
                    ),
                }
            }
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
            // DELETE answers 200 for an unknown id without deleting anything,
            // so without this check deleting by the wrong id (the rule's
            // version id is the easy mistake) reports success while the rule
            // stays put. Confirm it resolves before claiming we removed it.
            let target = if rule_found(&api.get(&id).await?) {
                id.clone()
            } else {
                // The miss might be a version id - one extra `list` call maps it
                // to the addressable id so we can delete the rule the user meant
                // instead of failing on a technicality.
                match classify_rule_id(&api, &id).await? {
                    RuleIdKind::VersionId(uid) => {
                        warn_version_id_autocorrected(&id, &uid);
                        uid
                    }
                    RuleIdKind::Addressable => id.clone(),
                    RuleIdKind::Unknown => bail!(
                        "No suppression rule found with ID '{id}'. This must be the rule's \
                         uniqueIdentifier, not its version id - run \
                         `cx alerts suppression-rules list` to find it."
                    ),
                }
            };
            api.delete(&target).await?;
            Ok(target)
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
