use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use tabled::{Table, Tabled};
use toon_format::encode_default as toon_encode;

use crate::api::alerts::{AlertDef, AlertsApi};
use crate::config::OutputFormat;
use crate::error::CxError;
use crate::execution::{fan_out, ExecutionTarget};

// ── Text-output row types ─────────────────────────────────────────────────────

#[derive(Tabled)]
struct AlertRow {
    #[tabled(rename = "Profile")]
    profile: String,
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    alert_type: String,
    #[tabled(rename = "Priority")]
    priority: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Updated")]
    updated: String,
}

#[derive(Tabled)]
struct AlertRowSingle {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    alert_type: String,
    #[tabled(rename = "Priority")]
    priority: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Updated")]
    updated: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bool_display(v: Option<bool>) -> String {
    match v {
        Some(true) => "yes".to_string(),
        Some(false) => "no".to_string(),
        None => "-".to_string(),
    }
}

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
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&all_json)?);
        }
        OutputFormat::Agents => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                println!("{}", "No alerts found.".yellow());
                return Ok(());
            }
            if include_profile {
                let rows: Vec<AlertRow> = all_items
                    .iter()
                    .map(|(profile, alert)| AlertRow {
                        profile: profile.clone(),
                        id: alert.id.clone().unwrap_or_default(),
                        name: alert.display_name(),
                        alert_type: alert.display_type(),
                        priority: alert.display_priority(),
                        enabled: bool_display(alert.display_enabled()),
                        status: alert.status.clone().unwrap_or_default(),
                        updated: alert.updated_time.clone().unwrap_or_default(),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            } else {
                let rows: Vec<AlertRowSingle> = all_items
                    .iter()
                    .map(|(_, alert)| AlertRowSingle {
                        id: alert.id.clone().unwrap_or_default(),
                        name: alert.display_name(),
                        alert_type: alert.display_type(),
                        priority: alert.display_priority(),
                        enabled: bool_display(alert.display_enabled()),
                        status: alert.status.clone().unwrap_or_default(),
                        updated: alert.updated_time.clone().unwrap_or_default(),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            }
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

    // Merge — collect raw API responses
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(mut val) => {
                if include_profile {
                    if let Value::Object(ref mut m) = val {
                        m.insert("_profile".to_string(), Value::String(profile.clone()));
                    }
                }
                all_results.push(val);
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    // Render
    match output {
        OutputFormat::Json => {
            if all_results.len() == 1 {
                println!("{}", serde_json::to_string_pretty(&all_results[0])?);
            } else {
                println!("{}", serde_json::to_string_pretty(&all_results)?);
            }
        }
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_results.is_empty() {
                println!("{}", "Alert not found.".yellow());
                return Ok(());
            }
            for val in &all_results {
                if include_profile {
                    if let Some(p) = val.get("_profile").and_then(|v| v.as_str()) {
                        println!("{}", format!("[{p}]").dimmed());
                    }
                }
                // Try to extract AlertDef for the human-friendly summary header
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
                            bool_display(alert.display_enabled())
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
                        println!();
                    }
                }
                println!("{}", serde_json::to_string_pretty(val)?);
            }
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

    let body: Value = serde_json::from_str(&raw)?;

    // Validate that alertDefProperties exists
    if body.get("alertDefProperties").is_none() {
        bail!("JSON must contain an 'alertDefProperties' key. See `cx alerts create --help`.");
    }

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
    let mut all_results: Vec<(String, Value)> = Vec::new();
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
                    all_results.push((profile, json));
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
        OutputFormat::Json => {
            if all_results.len() == 1 {
                println!("{}", serde_json::to_string_pretty(&all_results[0].1)?);
            } else {
                let vals: Vec<&Value> = all_results.iter().map(|(_, v)| v).collect();
                println!("{}", serde_json::to_string_pretty(&vals)?);
            }
        }
        OutputFormat::Agents => {
            let vals: Vec<&Value> = all_results.iter().map(|(_, v)| v).collect();
            let toon =
                toon_encode(&vals).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            // Status messages already printed to stderr above
        }
    }

    Ok(())
}

pub async fn run_enable(targets: &[Arc<ExecutionTarget>], alert_id: &str) -> Result<()> {
    eprintln!("{}", format!("Enabling alert {alert_id}...").dimmed());

    let id = alert_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = AlertsApi::new(&t.client);
            api.set_active(&id, true).await?;
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
            api.set_active(&id, false).await?;
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
