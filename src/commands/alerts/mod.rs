use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

pub mod api;

use crate::config::OutputFormat;
use crate::error::CxError;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{normalize_alert_payload, AlertDef, AlertsApi};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn alert_to_json(alert: &AlertDef, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": alert.id,
        "name": alert.display_name(),
        "description": alert.display_description(),
        "type": alert.display_type(),
        "priority": alert.display_priority(),
        "enabled": alert.display_enabled(),
        "status": alert.status,
        "created_time": alert.created_time,
        "updated_time": alert.updated_time,
        "last_triggered_time": alert.last_triggered_time,
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

fn event_field(event: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(value) = event.get(*key) {
            if let Some(s) = value.as_str() {
                return s.to_string();
            }
            if value.is_number() || value.is_boolean() {
                return value.to_string();
            }
        }
    }
    String::new()
}

fn event_to_json(event: &Value, include_profile: bool, profile: &str) -> Value {
    let mut v = event.clone();
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

fn next_page_token(pagination: Option<&Value>) -> Option<String> {
    pagination
        .and_then(|p| {
            p.get("nextPageToken")
                .or_else(|| p.get("next_page_token"))
                .or_else(|| p.get("next_page"))
        })
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    name_filter: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching alerts...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = AlertsApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    // Merge & filter
    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, AlertDef)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        let matching: Vec<AlertDef> = resp
            .alert_defs
            .into_iter()
            .filter(|alert| match name_filter {
                Some(filter) => alert
                    .display_name()
                    .to_lowercase()
                    .contains(&filter.to_lowercase()),
                None => true,
            })
            .collect();
        // Print the alerts list page link to stderr once per profile. Skip
        // when nothing matched (post-filter), since there's nothing to view.
        if !matching.is_empty() {
            crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                crate::console_url::alerts_url(b)
            })
            .await;
        }
        for alert in matching {
            let json = alert_to_json(&alert, include_profile, &profile);
            all_json.push(json);
            all_items.push((profile.clone(), alert));
        }
    }

    // Render
    match output {
        OutputFormat::Json => render::render_json(&all_json)?,
        OutputFormat::Toon => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                render::print_no_results("No alerts found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, alert)| {
                    vec![
                        profile.clone(),
                        alert.id.clone().unwrap_or_default(),
                        alert.display_name(),
                        alert.display_type(),
                        alert.display_priority(),
                        render::bool_display(alert.display_enabled()),
                        alert.status.clone().unwrap_or_default(),
                        alert.updated_time.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &[
                    "ID", "Name", "Type", "Priority", "Enabled", "Status", "Updated",
                ],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    alert_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching alert {alert_id}...").dimmed());

    let include_profile = targets.len() > 1;
    let id = alert_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = AlertsApi::new(&t.client);
            match api.get(&id).await {
                Ok(val) => Ok(val),
                Err(CxError::Api { status: 404, .. }) => Ok(api.get_by_version_id(&id).await?),
                Err(e) => Err(anyhow::Error::from(e)),
            }
        }
    })
    .await;

    // Merge - collect raw API responses
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::alert_url(b, alert_id)
        })
        .await;
        all_results.push(val);
    }

    // Render
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
                "Alert not found.",
                Some(&|val| {
                    if let Some(alert_def) = val.get("alertDef") {
                        if let Ok(alert) = serde_json::from_value::<AlertDef>(alert_def.clone()) {
                            println!("{}:        {}", "Name".bold(), alert.display_name());
                            println!(
                                "{}:          {}",
                                "ID".bold(),
                                alert.id.as_deref().unwrap_or("-")
                            );
                            println!("{}:        {}", "Type".bold(), alert.display_type());
                            println!("{}:    {}", "Priority".bold(), alert.display_priority());
                            println!(
                                "{}:     {}",
                                "Enabled".bold(),
                                render::bool_display(alert.display_enabled())
                            );
                            println!(
                                "{}:      {}",
                                "Status".bold(),
                                alert.status.as_deref().unwrap_or("-")
                            );
                            let desc = alert.display_description();
                            if !desc.is_empty() {
                                println!("{}: {}", "Description".bold(), desc);
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
    // Read JSON from file or stdin
    let raw = if from_file == "-" {
        eprintln!("{}", "Reading alert definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading alert definition from {from_file}...").dimmed()
        );
        std::fs::read_to_string(from_file)?
    };

    let mut body: Value = serde_json::from_str(&raw)?;

    // Validate that alertDefProperties exists
    if body.get("alertDefProperties").is_none() {
        bail!("JSON must contain an 'alertDefProperties' key. See `cx alerts create --help`.");
    }
    normalize_alert_payload(&mut body);

    eprintln!("{}", "Creating alert...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = AlertsApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    // Merge
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(alert) = resp.alert_def {
            let name = alert.display_name();
            render::print_created(
                "Created",
                "alert",
                Some(&name),
                alert.id.as_deref(),
                &profile,
            );
            if let Some(id) = alert.id.as_deref() {
                crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                    crate::console_url::alert_url(b, id)
                })
                .await;
            }
            let json = alert_to_json(&alert, include_profile, &profile);
            all_results.push(json);
        } else {
            eprintln!(
                "{}",
                format!("Alert created in profile '{profile}' but response was empty.").yellow()
            );
        }
    }

    // Render
    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            // Status messages already printed to stderr above
        }
    }

    Ok(())
}

/// Build params for alert update operation from user inputs.
fn build_alert_update_args<'a>(
    alert_id: Option<&'a str>,
    body: &'a Value,
) -> Result<(&'a str, &'a Value)> {
    if !body.is_object() {
        bail!("Alert definition must be a JSON object");
    }

    // If called with a previously-exported alert def, it'll be inside an
    // `alertDef` key.
    let alert_def = body.get("alertDef").unwrap_or(body);

    // Prefer the alert ID from the CLI arg if given, otherwise try to get
    // it from the alert definition.
    let Some(alert_id) = alert_id.or(alert_def.get("id").and_then(Value::as_str)) else {
        bail!("Can't determine the alert id")
    };

    // We need the `alertDefProperties`, bail if it's not there.
    let Some(alert_def_properties) = alert_def.get("alertDefProperties") else {
        bail!("Alert definition must contain an alertDefProperties")
    };

    Ok((alert_id, alert_def_properties))
}

pub async fn run_replace(
    targets: &[Arc<ExecutionTarget>],
    alert_id: Option<&str>,
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    // Read JSON from file or stdin
    let raw = if from_file == "-" {
        eprintln!("{}", "Reading alert definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading alert definition from {from_file}...").dimmed()
        );
        std::fs::read_to_string(from_file)?
    };

    let body: Value = serde_json::from_str(&raw)?;

    let (alert_id, alert_def_properties) = build_alert_update_args(alert_id, &body)?;

    let mut body = json!({
        "id": alert_id,
        "alertDefProperties": alert_def_properties
    });
    normalize_alert_payload(&mut body);

    // The API says `name` could be `None`, whereas `alert-schemas.md` implies
    // that it _must_ be present. For update it doesn't matter: either we're
    // updating the name or not. Let the other end decide what to do with it.
    let alert_name = alert_def_properties
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>")
        .to_string();
    eprintln!("{}", format!("Replacing alert {alert_name}...").dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = AlertsApi::new(&t.client);
            Ok(api.replace(&body).await?)
        }
    })
    .await;

    // Merge
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(alert) = resp.alert_def {
            let name = alert.display_name();
            render::print_created(
                "Replaced",
                "alert",
                Some(&name),
                alert.id.as_deref(),
                &profile,
            );
            if let Some(id) = alert.id.as_deref() {
                crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                    crate::console_url::alert_url(b, id)
                })
                .await;
            }
            let json = alert_to_json(&alert, include_profile, &profile);
            all_results.push(json);
        } else {
            eprintln!(
                "{}",
                format!("Alert replaced in profile '{profile}' but response was empty.").yellow()
            );
        }
    }

    // Render
    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            // Status messages already printed to stderr above
        }
    }

    Ok(())
}

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], alert_id: &str) -> Result<()> {
    eprintln!("{}", format!("Deleting alert {alert_id}...").dimmed());

    let id = alert_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = AlertsApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;

    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Deleted alert {alert_id} in profile '{profile}'.").green()
        );
    }

    Ok(())
}

fn replace_body_with_enabled(
    mut alert_response: Value,
    alert_id: &str,
    active: bool,
) -> Result<Value> {
    let alert_def = alert_response
        .get_mut("alertDef")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("Alert response did not contain 'alertDef'."))?;

    let mut properties = alert_def
        .remove("alertDefProperties")
        .ok_or_else(|| anyhow::anyhow!("Alert response did not contain 'alertDefProperties'."))?;

    let props = properties
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("'alertDefProperties' was not a JSON object."))?;
    props.insert("enabled".to_string(), Value::Bool(active));

    let mut body = json!({
        "id": alert_id,
        "alertDefProperties": properties,
    });
    normalize_alert_payload(&mut body);
    Ok(body)
}

pub async fn run_enable(targets: &[Arc<ExecutionTarget>], alert_id: &str) -> Result<()> {
    eprintln!("{}", format!("Enabling alert {alert_id}...").dimmed());

    let id = alert_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = AlertsApi::new(&t.client);
            let alert = api.get(&id).await?;
            let body = replace_body_with_enabled(alert, &id, true)?;
            api.replace(&body).await?;
            Ok(())
        }
    })
    .await;

    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Alert {alert_id} enabled in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::alert_url(b, alert_id)
        })
        .await;
    }

    Ok(())
}

pub async fn run_disable(targets: &[Arc<ExecutionTarget>], alert_id: &str) -> Result<()> {
    eprintln!("{}", format!("Disabling alert {alert_id}...").dimmed());

    let id = alert_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = AlertsApi::new(&t.client);
            let alert = api.get(&id).await?;
            let body = replace_body_with_enabled(alert, &id, false)?;
            api.replace(&body).await?;
            Ok(())
        }
    })
    .await;

    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Alert {alert_id} disabled in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::alert_url(b, alert_id)
        })
        .await;
    }

    Ok(())
}

pub async fn run_events(
    targets: &[Arc<ExecutionTarget>],
    alert_version_ids: &[String],
    start: Option<&str>,
    end: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching alert events...".dimmed());

    let include_profile = targets.len() > 1;
    let alert_version_ids = alert_version_ids.to_vec();
    let start_owned = start.unwrap_or("now-24h").to_string();
    let end_owned = end.unwrap_or("now").to_string();

    let per_profile = fan_out(targets, |t| {
        let alert_version_ids = alert_version_ids.clone();
        let start = start_owned.clone();
        let end = end_owned.clone();
        async move {
            let api = AlertsApi::new(&t.client);
            let start = crate::time::parse_timestamp(&start)?;
            let end = crate::time::parse_timestamp(&end)?;
            let mut all_events = Vec::new();
            let mut page_token: Option<String> = None;

            loop {
                let mut params: Vec<(&str, String)> = Vec::new();
                if alert_version_ids.is_empty() {
                    params.push(("filter.timestamp.from", start.clone()));
                    params.push(("filter.timestamp.to", end.clone()));
                    params.push(("pagination.page_size", "100".to_string()));
                } else {
                    for id in &alert_version_ids {
                        params.push(("alert_ids", id.clone()));
                    }
                    params.push(("timestamp_range.from", start.clone()));
                    params.push(("timestamp_range.to", end.clone()));
                }
                if let Some(token) = &page_token {
                    params.push(("pagination.page_token", token.clone()));
                }

                let params_ref: Vec<(&str, &str)> =
                    params.iter().map(|(k, v)| (*k, v.as_str())).collect();

                let resp = if alert_version_ids.is_empty() {
                    api.list_events(&params_ref).await?
                } else {
                    api.list_alert_events(&params_ref).await?
                };
                let next = next_page_token(resp.pagination.as_ref());
                all_events.extend(if resp.events.is_empty() {
                    resp.alert_events
                } else {
                    resp.events
                });

                match next {
                    Some(next) if page_token.as_deref() != Some(next.as_str()) => {
                        page_token = Some(next);
                    }
                    _ => break,
                }
            }
            Ok(all_events)
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, Value)> = Vec::new();
    for (profile, events) in report_errors_and_collect_successes(per_profile)? {
        for event in events {
            all_json.push(event_to_json(&event, include_profile, &profile));
            all_items.push((profile.clone(), event));
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
                render::print_no_results("No alert events found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, event)| {
                    vec![
                        profile.clone(),
                        event_field(event, &["cxEventKey", "id", "eventId"]),
                        event_field(event, &["cxEventType", "type", "status"]),
                        event_field(event, &["cxEventPayloadType", "alertName", "alert_name"]),
                        event_field(event, &["cxEventTimestamp", "triggeredAt", "triggered_at"]),
                        event_field(event, &["cxEventDedupKey", "dedupKey"]),
                    ]
                })
                .collect();
            render::render_table(
                &[
                    "Event Key",
                    "Type",
                    "Payload Type",
                    "Timestamp",
                    "Dedup Key",
                ],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}

pub async fn run_event_stats(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching alert event statistics...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = AlertsApi::new(&t.client);
        Ok(api.event_stats().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for mut stat in resp.stats {
            if include_profile {
                if let Value::Object(ref mut m) = stat {
                    m.insert("profile".to_string(), Value::String(profile.clone()));
                }
            }
            all_json.push(stat);
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
                render::print_no_results("No alert event statistics found.");
                return Ok(());
            }
            for val in &all_json {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// User is using a file they downloaded and edited.
    #[test]
    fn alert_properties_from_cli_args_from_previous_get() {
        let alert_id = None;
        let body = json!({
            "alertDef": {
                "alertDefProperties": {
                    "name": "my alert downloaded with GET",
                    "description": "other stuff below..."
                },
                "id": "123e4567-e89b-12d3-a456-426614174000"
            }
        });

        let (actual_id, actual_payload) =
            build_alert_update_args(alert_id, &body).expect("there should be no errors");

        assert_eq!(
            actual_id, "123e4567-e89b-12d3-a456-426614174000",
            "alert_id should be found in the alert definition body"
        );
        assert_eq!(
            *actual_payload,
            json!({
                "name": "my alert downloaded with GET",
                "description": "other stuff below..."
            }),
            "actual payload should be content of alertDefProperties"
        );
    }

    /// User is providing new body content with separate alert id to be replaced.
    #[test]
    fn alert_properties_from_cli_args_hand_crafted() {
        let alert_id = Some("123e4567-e89b-12d3-a456-426614174000");
        let body = json!({
            "alertDefProperties": {
                "name": "my alert downloaded with GET",
                "description": "other stuff below..."
            }
        });

        let (actual_id, actual_payload) =
            build_alert_update_args(alert_id, &body).expect("there should be no errors");

        assert_eq!(
            actual_id, "123e4567-e89b-12d3-a456-426614174000",
            "alert_id should be from CLI arg"
        );
        assert_eq!(
            *actual_payload,
            json!({
                "name": "my alert downloaded with GET",
                "description": "other stuff below..."
            }),
            "actual payload should be content of alertDefProperties"
        );
    }

    /// Alert ID from CLI should be preferred over that from file/stdin.
    #[test]
    fn alert_properties_from_cli_args_preferred() {
        let alert_id = Some("123e4567-e89b-12d3-a456-426614174000");
        let body = json!({
            "alertDef": {
                "alertDefProperties": {
                    "name": "my alert downloaded with GET",
                    "description": "other stuff below..."
                },
                "id": "11111111-2222-3333-4444-555555555555"
            }
        });

        let (actual_id, actual_payload) =
            build_alert_update_args(alert_id, &body).expect("there should be no errors");

        assert_eq!(
            actual_id, "123e4567-e89b-12d3-a456-426614174000",
            "alert_id should prefer CLI arg"
        );
        assert_eq!(
            *actual_payload,
            json!({
                "name": "my alert downloaded with GET",
                "description": "other stuff below..."
            }),
            "actual payload should be content of alertDefProperties"
        );
    }

    /// There must _be_ an Alert ID somewhere.
    #[test]
    fn alert_id_must_exist() {
        let alert_id = None;
        let body = json!({
            "alertDefProperties": {
                "name": "my alert downloaded with GET",
                "description": "other stuff below..."
            },
        });

        match build_alert_update_args(alert_id, &body) {
            Ok(_) => panic!("there should be an error"),
            Err(e) if e.to_string() != "Can't determine the alert id" => {
                panic!("Didn't get expected error")
            }
            _ => (),
        }
    }
}
