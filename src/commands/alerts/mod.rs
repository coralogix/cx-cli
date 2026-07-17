use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

pub mod api;

use crate::config::OutputFormat;
use crate::error::CxError;
use crate::execution::{fan_out, ExecutionTarget};
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
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for alert in resp.alert_defs {
                    if let Some(filter) = name_filter {
                        let name = alert.display_name().to_lowercase();
                        if !name.contains(&filter.to_lowercase()) {
                            continue;
                        }
                    }
                    all_json.push(alert_to_json(&alert, include_profile, &profile));
                    all_items.push((profile.clone(), alert));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    // Render
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

    // Render
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
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                if let Some(alert) = resp.alert_def {
                    let name = alert.display_name();
                    let id = alert.id.as_deref().unwrap_or("unknown");
                    eprintln!(
                        "{}",
                        format!("Created alert '{name}' (ID: {id}) in profile '{profile}'.")
                            .green()
                    );
                    let json = alert_to_json(&alert, include_profile, &profile);
                    all_results.push(json);
                } else {
                    eprintln!(
                        "{}",
                        format!("Alert created in profile '{profile}' but response was empty.")
                            .yellow()
                    );
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    // Render
    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Yaml => render::render_yaml_auto(&all_results)?,
        OutputFormat::Agents => {
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

    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!("Deleted alert {alert_id} in profile '{profile}'.").green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
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

    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!("Alert {alert_id} enabled in profile '{profile}'.").green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
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

    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!("Alert {alert_id} disabled in profile '{profile}'.").green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
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
    for (profile, result) in per_profile {
        match result {
            Ok(events) => {
                for event in events {
                    all_json.push(event_to_json(&event, include_profile, &profile));
                    all_items.push((profile.clone(), event));
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
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for mut stat in resp.stats {
                    if include_profile {
                        if let Value::Object(ref mut m) = stat {
                            m.insert("profile".to_string(), Value::String(profile.clone()));
                        }
                    }
                    all_json.push(stat);
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
