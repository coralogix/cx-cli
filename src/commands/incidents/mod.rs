pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;
use api::{Incident, IncidentsApi};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn incident_to_json(incident: &Incident, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": incident.id,
        "name": incident.name,
        "severity": incident.display_severity(),
        "state": incident.display_state(),
        "created_at": incident.created_at,
        "closed_at": incident.closed_at,
        "is_muted": incident.is_muted,
        "assigned_to": incident.display_assignees(),
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
    status_filter: Option<&str>,
    severity_filter: Option<&str>,
    assignee_filter: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching incidents...".dimmed());

    let include_profile = targets.len() > 1;

    let status_owned = status_filter.map(|s| s.to_string());
    let severity_owned = severity_filter.map(|s| s.to_string());
    let assignee_owned = assignee_filter.map(|s| s.to_string());

    let per_profile = fan_out(targets, |t| {
        let status = status_owned.clone();
        let severity = severity_owned.clone();
        let assignee = assignee_owned.clone();
        async move {
            let api = IncidentsApi::new(&t.client);
            let mut filter = json!({});
            if let Some(s) = status {
                filter["status"] = Value::String(s);
            }
            if let Some(s) = severity {
                filter["severity"] = Value::String(s);
            }
            if let Some(a) = assignee {
                filter["assignee"] = Value::String(a);
            }
            let body = json!({ "filter": filter });
            Ok(api.list(&body).await?)
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, Incident)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for incident in resp.incidents {
                    all_json.push(incident_to_json(&incident, include_profile, &profile));
                    all_items.push((profile.clone(), incident));
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
                render::print_no_results("No incidents found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, inc)| {
                    vec![
                        profile.clone(),
                        inc.id.clone().unwrap_or_default(),
                        inc.name.clone().unwrap_or_default(),
                        inc.display_severity(),
                        inc.display_state(),
                        inc.created_at.clone().unwrap_or_default(),
                        inc.display_assignees(),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Severity", "State", "Created", "Assigned To"],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    incident_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching incident {incident_id}...").dimmed());

    let include_profile = targets.len() > 1;
    let id = incident_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = IncidentsApi::new(&t.client);
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
                "Incident not found.",
                Some(&|val| {
                    if let Some(incident_val) = val.get("incident") {
                        if let Ok(inc) = serde_json::from_value::<Incident>(incident_val.clone()) {
                            println!(
                                "{}:      {}",
                                "Name".bold(),
                                inc.name.as_deref().unwrap_or("-")
                            );
                            println!(
                                "{}:        {}",
                                "ID".bold(),
                                inc.id.as_deref().unwrap_or("-")
                            );
                            println!("{}:  {}", "Severity".bold(), inc.display_severity());
                            println!("{}:     {}", "State".bold(), inc.display_state());
                            println!(
                                "{}:   {}",
                                "Created".bold(),
                                inc.created_at.as_deref().unwrap_or("-")
                            );
                            println!("{}:  {}", "Assigned".bold(), inc.display_assignees());
                            if let Some(desc) = inc.description.as_deref() {
                                if !desc.is_empty() {
                                    println!("{}: {}", "Description".bold(), desc);
                                }
                            }
                        }
                    }
                }),
            )?;
        }
    }

    Ok(())
}

pub async fn run_acknowledge(
    targets: &[Arc<ExecutionTarget>],
    incident_ids: &[String],
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Acknowledging {} incident(s)...", incident_ids.len()).dimmed()
    );

    let ids = incident_ids.to_vec();

    let per_profile = fan_out(targets, |t| {
        let ids = ids.clone();
        async move {
            let api = IncidentsApi::new(&t.client);
            api.acknowledge(&ids).await?;
            Ok(())
        }
    })
    .await;

    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!(
                    "Acknowledged {} incident(s) in profile '{profile}'.",
                    incident_ids.len()
                )
                .green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    Ok(())
}

pub async fn run_resolve(targets: &[Arc<ExecutionTarget>], incident_ids: &[String]) -> Result<()> {
    eprintln!(
        "{}",
        format!("Resolving {} incident(s)...", incident_ids.len()).dimmed()
    );

    let ids = incident_ids.to_vec();

    let per_profile = fan_out(targets, |t| {
        let ids = ids.clone();
        async move {
            let api = IncidentsApi::new(&t.client);
            api.resolve(&ids).await?;
            Ok(())
        }
    })
    .await;

    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!(
                    "Resolved {} incident(s) in profile '{profile}'.",
                    incident_ids.len()
                )
                .green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    Ok(())
}

pub async fn run_close(targets: &[Arc<ExecutionTarget>], incident_ids: &[String]) -> Result<()> {
    eprintln!(
        "{}",
        format!("Closing {} incident(s)...", incident_ids.len()).dimmed()
    );

    let ids = incident_ids.to_vec();

    let per_profile = fan_out(targets, |t| {
        let ids = ids.clone();
        async move {
            let api = IncidentsApi::new(&t.client);
            api.close(&ids).await?;
            Ok(())
        }
    })
    .await;

    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!(
                    "Closed {} incident(s) in profile '{profile}'.",
                    incident_ids.len()
                )
                .green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    Ok(())
}

pub async fn run_assign(
    targets: &[Arc<ExecutionTarget>],
    incident_ids: &[String],
    user_id: &str,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Assigning {} incident(s) to user", incident_ids.len()).dimmed()
    );

    let ids = incident_ids.to_vec();
    let uid = user_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let ids = ids.clone();
        let uid = uid.clone();
        async move {
            let api = IncidentsApi::new(&t.client);
            api.assign(&ids, &uid).await?;
            Ok(())
        }
    })
    .await;

    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!(
                    "Assigned {} incident(s) to user in profile '{profile}'.",
                    incident_ids.len()
                )
                .green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    Ok(())
}

pub async fn run_unassign(targets: &[Arc<ExecutionTarget>], incident_ids: &[String]) -> Result<()> {
    eprintln!(
        "{}",
        format!("Unassigning {} incident(s)...", incident_ids.len()).dimmed()
    );

    let ids = incident_ids.to_vec();

    let per_profile = fan_out(targets, |t| {
        let ids = ids.clone();
        async move {
            let api = IncidentsApi::new(&t.client);
            api.unassign(&ids).await?;
            Ok(())
        }
    })
    .await;

    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!(
                    "Unassigned {} incident(s) in profile '{profile}'.",
                    incident_ids.len()
                )
                .green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    Ok(())
}

pub async fn run_events(
    targets: &[Arc<ExecutionTarget>],
    incident_id: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching incident events...".dimmed());

    let include_profile = targets.len() > 1;
    let id_owned = incident_id.map(|s| s.to_string());

    let per_profile = fan_out(targets, |t| {
        let id = id_owned.clone();
        async move {
            let api = IncidentsApi::new(&t.client);
            Ok(api.list_events(id.as_deref()).await?)
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, api::IncidentEvent)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for event in resp.events {
                    let mut v = json!({
                        "id": event.id,
                        "incident_id": event.incident_id,
                        "type": event.event_type,
                        "created_at": event.created_at,
                    });
                    if include_profile {
                        if let Value::Object(ref mut m) = v {
                            m.insert("profile".to_string(), Value::String(profile.clone()));
                        }
                    }
                    all_json.push(v);
                    all_items.push((profile.clone(), event));
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
                render::print_no_results("No incident events found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, evt)| {
                    vec![
                        profile.clone(),
                        evt.id.clone().unwrap_or_default(),
                        evt.incident_id.clone().unwrap_or_default(),
                        evt.event_type.clone().unwrap_or_default(),
                        evt.created_at.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["Event ID", "Incident ID", "Type", "Created"],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}

pub async fn run_aggregations(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching incident aggregations...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = IncidentsApi::new(&t.client);
        Ok(api.aggregations().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for mut agg in resp.aggregations {
                    if include_profile {
                        if let Value::Object(ref mut m) = agg {
                            m.insert("profile".to_string(), Value::String(profile.clone()));
                        }
                    }
                    all_json.push(agg);
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
            if all_json.is_empty() {
                render::print_no_results("No incident aggregations found.");
                return Ok(());
            }
            for val in &all_json {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}
