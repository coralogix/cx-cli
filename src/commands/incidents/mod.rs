pub mod api;

use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;
use api::{Incident, IncidentsApi};

#[derive(Debug, Clone, Default)]
pub struct ListIncidentsOptions {
    pub statuses: Vec<String>,
    pub severities: Vec<String>,
    pub states: Vec<String>,
    pub assignees: Vec<String>,
    pub application_names: Vec<String>,
    pub subsystem_names: Vec<String>,
    pub contextual_labels: Vec<String>,
    pub search_query: Option<String>,
    pub search_field: Option<String>,
    pub search_contextual_label: Option<String>,
    pub is_muted: Option<bool>,
    pub created_start: Option<String>,
    pub created_end: Option<String>,
    pub duration_start: Option<String>,
    pub duration_end: Option<String>,
    pub order_by: Option<String>,
    pub order_direction: Option<String>,
    pub page_size: u32,
    pub page_token: Option<String>,
    pub limit: Option<usize>,
}

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

fn normalize_enum(input: &str, prefix: &str) -> String {
    let normalized = input.trim().replace(['-', ' '], "_").to_ascii_uppercase();
    if normalized.starts_with(prefix) {
        normalized
    } else {
        format!("{prefix}{normalized}")
    }
}

fn normalize_incident_field(input: &str) -> String {
    let normalized = input.trim().replace(['-', ' '], "_").to_ascii_uppercase();
    let field = match normalized.as_str() {
        "CREATED" | "CREATED_AT" | "CREATED_TIME" => "CREATED_TIME",
        "CLOSED" | "CLOSED_AT" | "CLOSED_TIME" => "CLOSED_TIME",
        "LAST_UPDATE" | "LAST_STATE_UPDATE" | "LAST_STATE_UPDATE_AT" | "LAST_STATE_UPDATE_TIME" => {
            "LAST_STATE_UPDATE_TIME"
        }
        "APPLICATION" | "APPLICATION_NAME" => "APPLICATION_NAME",
        "SUBSYSTEM" | "SUBSYSTEM_NAME" => "SUBSYSTEM_NAME",
        other => other,
    };

    if field.starts_with("INCIDENTS_FIELDS_") {
        field.to_string()
    } else {
        format!("INCIDENTS_FIELDS_{field}")
    }
}

fn normalize_order_direction(input: &str) -> String {
    normalize_enum(input, "ORDER_BY_DIRECTION_")
}

fn insert_non_empty_array(filter: &mut Value, key: &str, values: &[String], prefix: Option<&str>) {
    if values.is_empty() {
        return;
    }

    let items: Vec<Value> = values
        .iter()
        .map(|v| match prefix {
            Some(prefix) => Value::String(normalize_enum(v, prefix)),
            None => Value::String(v.clone()),
        })
        .collect();
    filter[key] = Value::Array(items);
}

fn insert_time_range(
    filter: &mut Value,
    key: &str,
    start: Option<&str>,
    end: Option<&str>,
) -> Result<()> {
    if start.is_none() && end.is_none() {
        return Ok(());
    }

    let mut range = json!({});
    if let Some(start) = start {
        range["startTime"] = Value::String(crate::time::parse_timestamp(start)?);
    }
    if let Some(end) = end {
        range["endTime"] = Value::String(crate::time::parse_timestamp(end)?);
    }
    filter[key] = range;

    Ok(())
}

fn insert_contextual_labels(filter: &mut Value, labels: &[String]) -> Result<()> {
    if labels.is_empty() {
        return Ok(());
    }

    let mut map = serde_json::Map::new();
    for label in labels {
        let Some((key, value)) = label.split_once('=') else {
            bail!("Invalid contextual label '{label}'. Use key=value.");
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            bail!("Invalid contextual label '{label}'. Use key=value.");
        }

        let entry = map
            .entry(key.to_string())
            .or_insert_with(|| json!({ "contextualLabelValues": [] }));
        entry["contextualLabelValues"]
            .as_array_mut()
            .expect("contextual label values initialized as array")
            .push(Value::String(value.to_string()));
    }

    filter["contextualLabels"] = Value::Object(map);
    Ok(())
}

fn build_list_body(options: &ListIncidentsOptions, page_token: Option<&str>) -> Result<Value> {
    let mut filter = json!({});

    insert_non_empty_array(
        &mut filter,
        "status",
        &options.statuses,
        Some("INCIDENT_STATUS_"),
    );
    insert_non_empty_array(
        &mut filter,
        "severity",
        &options.severities,
        Some("INCIDENT_SEVERITY_"),
    );
    insert_non_empty_array(
        &mut filter,
        "state",
        &options.states,
        Some("INCIDENT_STATE_"),
    );
    insert_non_empty_array(&mut filter, "assignee", &options.assignees, None);
    insert_non_empty_array(
        &mut filter,
        "applicationName",
        &options.application_names,
        None,
    );
    insert_non_empty_array(&mut filter, "subsystemName", &options.subsystem_names, None);
    insert_contextual_labels(&mut filter, &options.contextual_labels)?;

    if let Some(is_muted) = options.is_muted {
        filter["isMuted"] = Value::Bool(is_muted);
    }

    if let Some(query) = options.search_query.as_deref() {
        if options.search_field.is_some() && options.search_contextual_label.is_some() {
            bail!("Use only one of --query-field or --query-contextual-label with --query.");
        }
        if options.search_field.is_none() && options.search_contextual_label.is_none() {
            bail!("Use --query-field or --query-contextual-label with --query.");
        }

        let mut search_query = json!({ "query": query });
        if let Some(field) = options.search_field.as_deref() {
            search_query["incidentField"] = Value::String(normalize_incident_field(field));
        }
        if let Some(label) = options.search_contextual_label.as_deref() {
            search_query["contextualLabel"] = Value::String(label.to_string());
        }
        filter["searchQuery"] = search_query;
    }

    insert_time_range(
        &mut filter,
        "createdAtRange",
        options.created_start.as_deref(),
        options.created_end.as_deref(),
    )?;
    insert_time_range(
        &mut filter,
        "incidentDurationRange",
        options.duration_start.as_deref(),
        options.duration_end.as_deref(),
    )?;

    let mut body = json!({
        "filter": filter,
        "pagination": {
            "pageSize": options.page_size,
        },
    });

    if let Some(token) = page_token.or(options.page_token.as_deref()) {
        body["pagination"]["pageToken"] = Value::String(token.to_string());
    }

    if let Some(order_by) = options.order_by.as_deref() {
        body["orderBys"] = json!([
            {
                "incidentField": normalize_incident_field(order_by),
                "direction": normalize_order_direction(
                    options.order_direction.as_deref().unwrap_or("DESC")
                ),
            }
        ]);
    }

    Ok(body)
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    options: ListIncidentsOptions,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching incidents...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let options = options.clone();
        async move {
            let api = IncidentsApi::new(&t.client);
            let mut incidents = Vec::new();
            let mut next_page_token: Option<String> = None;

            loop {
                let body = build_list_body(&options, next_page_token.as_deref())?;
                let mut resp = api.list(&body).await?;
                next_page_token = resp.next_page_token().map(ToString::to_string);
                incidents.append(&mut resp.incidents);

                if let Some(limit) = options.limit {
                    if incidents.len() >= limit {
                        incidents.truncate(limit);
                        break;
                    }
                }

                if next_page_token.is_none() {
                    break;
                }
            }

            Ok(incidents)
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, Incident)> = Vec::new();
    let mut success_count = 0usize;
    let mut error_count = 0usize;
    for (profile, result) in per_profile {
        match result {
            Ok(incidents) => {
                success_count += 1;
                for incident in incidents {
                    all_json.push(incident_to_json(&incident, include_profile, &profile));
                    all_items.push((profile.clone(), incident));
                }
            }
            Err(e) => {
                error_count += 1;
                eprintln!("{}", format!("error from profile '{profile}': {e:#}").red());
            }
        }
    }

    if success_count == 0 && error_count > 0 {
        bail!("Failed to list incidents for all selected profiles.");
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
        format!(
            "Assigning {} incident(s) to user {user_id}...",
            incident_ids.len()
        )
        .dimmed()
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
                    "Assigned {} incident(s) to {user_id} in profile '{profile}'.",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_list_body_uses_repeated_fields_and_full_enum_names() {
        let body = build_list_body(
            &ListIncidentsOptions {
                statuses: vec![
                    "triggered".to_string(),
                    "INCIDENT_STATUS_ACKNOWLEDGED".to_string(),
                ],
                severities: vec!["critical".to_string()],
                states: vec!["resolved".to_string()],
                assignees: vec!["user-1".to_string()],
                page_size: 25,
                limit: Some(25),
                ..Default::default()
            },
            None,
        )
        .unwrap();

        assert_eq!(
            body["filter"]["status"],
            json!(["INCIDENT_STATUS_TRIGGERED", "INCIDENT_STATUS_ACKNOWLEDGED"])
        );
        assert_eq!(
            body["filter"]["severity"],
            json!(["INCIDENT_SEVERITY_CRITICAL"])
        );
        assert_eq!(body["filter"]["state"], json!(["INCIDENT_STATE_RESOLVED"]));
        assert_eq!(body["filter"]["assignee"], json!(["user-1"]));
        assert_eq!(body["pagination"]["pageSize"], json!(25));
    }

    #[test]
    fn build_list_body_supports_search_labels_ordering_and_page_token() {
        let body = build_list_body(
            &ListIncidentsOptions {
                application_names: vec!["checkout".to_string()],
                subsystem_names: vec!["api".to_string()],
                contextual_labels: vec![
                    "team=payments".to_string(),
                    "team=platform".to_string(),
                    "env=prod".to_string(),
                ],
                search_query: Some("latency".to_string()),
                search_field: Some("name".to_string()),
                is_muted: Some(false),
                order_by: Some("created_at".to_string()),
                order_direction: Some("asc".to_string()),
                page_size: 50,
                limit: Some(50),
                ..Default::default()
            },
            Some("next-token"),
        )
        .unwrap();

        assert_eq!(body["filter"]["applicationName"], json!(["checkout"]));
        assert_eq!(body["filter"]["subsystemName"], json!(["api"]));
        assert_eq!(
            body["filter"]["contextualLabels"]["team"]["contextualLabelValues"],
            json!(["payments", "platform"])
        );
        assert_eq!(
            body["filter"]["contextualLabels"]["env"]["contextualLabelValues"],
            json!(["prod"])
        );
        assert_eq!(body["filter"]["searchQuery"]["query"], json!("latency"));
        assert_eq!(
            body["filter"]["searchQuery"]["incidentField"],
            json!("INCIDENTS_FIELDS_NAME")
        );
        assert_eq!(body["filter"]["isMuted"], json!(false));
        assert_eq!(body["pagination"]["pageToken"], json!("next-token"));
        assert_eq!(
            body["orderBys"],
            json!([
                {
                    "incidentField": "INCIDENTS_FIELDS_CREATED_TIME",
                    "direction": "ORDER_BY_DIRECTION_ASC"
                }
            ])
        );
    }

    #[test]
    fn invalid_contextual_label_errors() {
        let err = build_list_body(
            &ListIncidentsOptions {
                contextual_labels: vec!["team".to_string()],
                page_size: 100,
                limit: Some(100),
                ..Default::default()
            },
            None,
        )
        .unwrap_err();

        assert!(err.to_string().contains("Use key=value"));
    }

    #[test]
    fn search_query_requires_exactly_one_field_target() {
        let no_field = build_list_body(
            &ListIncidentsOptions {
                search_query: Some("error".to_string()),
                page_size: 100,
                limit: Some(100),
                ..Default::default()
            },
            None,
        )
        .unwrap_err();
        assert!(no_field.to_string().contains("--query-field"));

        let two_fields = build_list_body(
            &ListIncidentsOptions {
                search_query: Some("error".to_string()),
                search_field: Some("name".to_string()),
                search_contextual_label: Some("service".to_string()),
                page_size: 100,
                limit: Some(100),
                ..Default::default()
            },
            None,
        )
        .unwrap_err();
        assert!(two_fields.to_string().contains("Use only one"));
    }
}
