pub mod api;

use std::sync::Arc;

use anyhow::{anyhow, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;
use api::{Case, CasesApi, TeammateDirectory};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Normalize a CLI priority shorthand (e.g. "P1", "p2") into the enum value the
/// API expects ("CASE_PRIORITY_P1"). Pass-through if already prefixed.
pub fn normalize_priority(input: &str) -> Result<String> {
    let upper = input.trim().to_uppercase();
    if upper.starts_with("CASE_PRIORITY_") {
        return Ok(upper);
    }
    match upper.as_str() {
        "P1" | "P2" | "P3" | "P4" | "P5" => Ok(format!("CASE_PRIORITY_{upper}")),
        _ => Err(anyhow!(
            "invalid priority '{input}': expected one of P1, P2, P3, P4, P5"
        )),
    }
}

/// Insert (or replace) the `assignees` array inside the request body's
/// `filters` object. No-op if `filters` is missing or not an object.
fn inject_assignees_filter(body: &mut Value, entry: Value) {
    if let Some(filters) = body.get_mut("filters").and_then(|v| v.as_object_mut()) {
        filters.insert("assignees".to_string(), Value::Array(vec![entry]));
    }
}

fn normalize_enum_filter(values: &[String], prefix: &str) -> Vec<String> {
    values
        .iter()
        .map(|v| {
            let upper = v.trim().to_uppercase();
            if upper.starts_with(prefix) {
                upper
            } else {
                format!("{prefix}{upper}")
            }
        })
        .collect()
}

/// Resolve a case's assignee to a human-readable string (email when known,
/// otherwise raw user ID, otherwise "-").
fn assignee_display(case: &Case, directory: &TeammateDirectory) -> String {
    let Some(uid) = case.assignee.as_ref().and_then(|a| a.user_id.as_deref()) else {
        return "-".to_string();
    };
    directory.email_for(uid).unwrap_or(uid).to_string()
}

fn case_to_json(
    case: &Case,
    directory: &TeammateDirectory,
    include_profile: bool,
    profile: &str,
) -> Value {
    let assignee = case
        .assignee
        .as_ref()
        .and_then(|a| a.user_id.as_deref())
        .map(|uid| directory.email_for(uid).unwrap_or(uid).to_string());
    let mut v = json!({
        "id": case.id,
        "readable_id": case.readable_id,
        "title": case.title,
        "status": case.display_status(),
        "priority": case.display_priority(),
        "category": case.display_category(),
        "assignee": assignee,
        "create_time": case.create_time,
        "update_time": case.update_time,
        "acknowledge_time": case.acknowledge_time,
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

/// Rewrite the `assignee.userId` inside a raw case payload (as returned by
/// the API) to a `{ "email": ... }` shape so consumers never see opaque IDs.
/// When the directory has no entry, the original value is preserved as
/// `assignee.userId`.
fn substitute_assignee_email(value: &mut Value, directory: &TeammateDirectory) {
    let Some(assignee) = value.get_mut("assignee").and_then(|v| v.as_object_mut()) else {
        return;
    };
    let Some(uid) = assignee
        .get("userId")
        .and_then(|v| v.as_str())
        .map(String::from)
    else {
        return;
    };
    if let Some(email) = directory.email_for(&uid) {
        assignee.remove("userId");
        assignee.insert("email".to_string(), Value::String(email.to_string()));
    }
}

// ── Subcommand runners ────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    statuses: &[String],
    priorities: &[String],
    categories: &[String],
    assignee: Option<&str>,
    text: Option<&str>,
    page_size: Option<u32>,
    page_token: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching cases...".dimmed());

    let include_profile = targets.len() > 1;

    let mut filters = serde_json::Map::new();
    if !statuses.is_empty() {
        filters.insert(
            "statuses".to_string(),
            Value::Array(
                normalize_enum_filter(statuses, "CASE_STATUS_")
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if !priorities.is_empty() {
        filters.insert(
            "priorities".to_string(),
            Value::Array(
                normalize_enum_filter(priorities, "CASE_PRIORITY_")
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if !categories.is_empty() {
        filters.insert(
            "categories".to_string(),
            Value::Array(
                normalize_enum_filter(categories, "CASE_CATEGORY_")
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if let Some(t) = text {
        filters.insert("textSearch".to_string(), Value::String(t.to_string()));
    }

    let mut pagination = serde_json::Map::new();
    if let Some(size) = page_size {
        pagination.insert("pageSize".to_string(), json!(size));
    }
    if let Some(token) = page_token {
        pagination.insert("pageToken".to_string(), Value::String(token.to_string()));
    }

    let mut body = serde_json::Map::new();
    body.insert("filters".to_string(), Value::Object(filters));
    if !pagination.is_empty() {
        body.insert("pagination".to_string(), Value::Object(pagination));
    }
    let body = Value::Object(body);
    let assignee_owned: Option<String> = assignee.map(String::from);

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        let assignee = assignee_owned.clone();
        async move {
            let api = CasesApi::new(&t.client);
            // --assignee is resolved per profile because user IDs are
            // team-scoped — resolving once against the primary profile would
            // silently return wrong/empty results for the others.
            let is_email = assignee
                .as_deref()
                .is_some_and(|a| a.contains('@') && !a.eq_ignore_ascii_case("unassigned"));
            if is_email {
                // Email path: fetch directory first, then list with the
                // resolved user ID. Cannot race the two requests here.
                let email = assignee.as_deref().expect("checked above");
                let directory = api.teammate_directory().await.map_err(|e| {
                    anyhow!("failed to fetch teammates for --assignee resolution: {e:#}")
                })?;
                let uid = directory.resolve_to_user_id(email)?;
                let mut body = body;
                inject_assignees_filter(&mut body, json!({ "assignee": uid }));
                let cases = api.list(&body).await?;
                Ok::<_, anyhow::Error>((cases, directory))
            } else {
                // Non-email path: inject any pass-through filter and race
                // the directory fetch with the list call.
                let mut body = body;
                if let Some(a) = assignee.as_deref() {
                    let entry = if a.eq_ignore_ascii_case("unassigned") {
                        json!({ "unassigned": {} })
                    } else {
                        json!({ "assignee": a })
                    };
                    inject_assignees_filter(&mut body, entry);
                }
                // Fetch teammates in parallel; failure is non-fatal — we fall back
                // to raw user IDs in the output so the list call still produces
                // useful data.
                let (cases_res, dir_res) = tokio::join!(api.list(&body), api.teammate_directory());
                Ok::<_, anyhow::Error>((cases_res?, dir_res.unwrap_or_default()))
            }
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, Case, TeammateDirectory)> = Vec::new();
    let mut next_tokens: Vec<(String, String)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok((resp, directory)) => {
                if let Some(token) = resp.pagination.and_then(|p| p.next_page_token) {
                    next_tokens.push((profile.clone(), token));
                }
                for case in resp.cases {
                    all_json.push(case_to_json(&case, &directory, include_profile, &profile));
                    all_items.push((profile.clone(), case, directory.clone()));
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
                render::print_no_results("No cases found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, case, directory)| {
                    vec![
                        profile.clone(),
                        case.id.clone().unwrap_or_default(),
                        case.readable_id.clone().unwrap_or_default(),
                        case.title.clone().unwrap_or_default(),
                        case.display_status(),
                        case.display_priority(),
                        case.display_category(),
                        assignee_display(case, directory),
                        case.create_time.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &[
                    "ID", "Readable", "Title", "Status", "Priority", "Category", "Assignee",
                    "Created",
                ],
                rows,
                include_profile,
            );
            for (profile, token) in &next_tokens {
                eprintln!(
                    "{}",
                    format!("[{profile}] more results available; pass --page-token \"{token}\"")
                        .dimmed()
                );
            }
        }
    }

    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching case {case_id}...").dimmed());

    let include_profile = targets.len() > 1;
    let id = case_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = CasesApi::new(&t.client);
            let (case_res, dir_res) = tokio::join!(api.get(&id), api.teammate_directory());
            let case = case_res?;
            let directory = dir_res.unwrap_or_default();
            Ok::<_, anyhow::Error>((case, directory))
        }
    })
    .await;

    let mut all_results: Vec<(Value, TeammateDirectory)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok((mut val, directory)) => {
                if let Some(case) = val.get_mut("case") {
                    substitute_assignee_email(case, &directory);
                }
                if include_profile {
                    render::tag_get_result(&mut val, &profile);
                }
                all_results.push((val, directory));
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    // Most renderers want only the JSON values; the directory we keep for the
    // text summary callback below.
    let values: Vec<Value> = all_results.iter().map(|(v, _)| v.clone()).collect();
    match output {
        OutputFormat::Json => render::render_json_auto(&values)?,
        OutputFormat::Agents => {
            let toon =
                toon_encode(&values).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            render::render_get_text(
                &values,
                include_profile,
                "Case not found.",
                Some(&|val| {
                    if let Some(case_val) = val.get("case") {
                        if let Ok(case) = serde_json::from_value::<Case>(case_val.clone()) {
                            println!(
                                "{}:        {}",
                                "Title".bold(),
                                case.title.as_deref().unwrap_or("-")
                            );
                            println!(
                                "{}:           {}",
                                "ID".bold(),
                                case.id.as_deref().unwrap_or("-")
                            );
                            if let Some(rid) = case.readable_id.as_deref() {
                                println!("{}:     {rid}", "Readable ID".bold());
                            }
                            println!("{}:       {}", "Status".bold(), case.display_status());
                            println!("{}:     {}", "Priority".bold(), case.display_priority());
                            println!("{}:     {}", "Category".bold(), case.display_category());
                            // After substitute_assignee_email, the case JSON has
                            // assignee.email instead of assignee.userId when known.
                            let assignee = case_val
                                .get("assignee")
                                .and_then(|a| {
                                    a.get("email")
                                        .and_then(|v| v.as_str())
                                        .or_else(|| a.get("userId").and_then(|v| v.as_str()))
                                })
                                .unwrap_or("-");
                            println!("{}:     {assignee}", "Assignee".bold());
                            println!(
                                "{}:      {}",
                                "Created".bold(),
                                case.create_time.as_deref().unwrap_or("-")
                            );
                            if let Some(ack) = case.acknowledge_time.as_deref() {
                                println!("{}: {ack}", "Acknowledged".bold());
                            }
                            if let Some(summary) = case.ai_summary.as_deref() {
                                if !summary.is_empty() {
                                    println!("{}:   {summary}", "AI summary".bold());
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

pub async fn run_update(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    title: Option<&str>,
    resolution_reason: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    if title.is_none() && resolution_reason.is_none() {
        return Err(anyhow!(
            "specify at least one of --title or --resolution-reason"
        ));
    }

    eprintln!("{}", format!("Updating case {case_id}...").dimmed());

    let mut patch = serde_json::Map::new();
    if let Some(t) = title {
        patch.insert("title".to_string(), Value::String(t.to_string()));
    }
    if let Some(r) = resolution_reason {
        patch.insert("resolutionReason".to_string(), Value::String(r.to_string()));
    }
    let patch = Value::Object(patch);
    let id = case_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let patch = patch.clone();
        let id = id.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.update(&id, &patch).await?)
        }
    })
    .await;

    finish_lifecycle(
        per_profile,
        targets.len() > 1,
        output,
        &format!("Updated case '{case_id}'"),
    )
}

pub async fn run_assign(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    user: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Assigning case {case_id} to {user}...").dimmed()
    );

    let id = case_id.to_string();
    let input = user.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        let input = input.clone();
        async move {
            let api = CasesApi::new(&t.client);
            // If the input is an email, resolve to a user ID via the team
            // directory; otherwise pass through as already being a user ID.
            // The directory call is only made when needed, since most agent
            // workflows already have the user ID in hand.
            let user_id = if input.contains('@') {
                let directory = api.teammate_directory().await.map_err(|e| {
                    anyhow!("failed to fetch teammates to resolve email '{input}': {e:#}")
                })?;
                directory.resolve_to_user_id(&input)?
            } else {
                input.clone()
            };
            let mut result = api.assign(&id, &user_id).await?;
            // Surface the email in the response so the user never sees the userId.
            let directory = api.teammate_directory().await.unwrap_or_default();
            if let Some(case) = result.get_mut("case") {
                substitute_assignee_email(case, &directory);
            }
            Ok::<_, anyhow::Error>(result)
        }
    })
    .await;

    finish_lifecycle(
        per_profile,
        targets.len() > 1,
        output,
        &format!("Assigned case '{case_id}' to '{user}'"),
    )
}

pub async fn run_unassign(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Unassigning case {case_id}...").dimmed());
    let id = case_id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.unassign(&id).await?)
        }
    })
    .await;
    finish_lifecycle(
        per_profile,
        targets.len() > 1,
        output,
        &format!("Unassigned case '{case_id}'"),
    )
}

pub async fn run_acknowledge(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Acknowledging case {case_id}...").dimmed());
    let id = case_id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.acknowledge(&id).await?)
        }
    })
    .await;
    finish_lifecycle(
        per_profile,
        targets.len() > 1,
        output,
        &format!("Acknowledged case '{case_id}'"),
    )
}

pub async fn run_unacknowledge(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Unacknowledging case {case_id}...").dimmed());
    let id = case_id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.unacknowledge(&id).await?)
        }
    })
    .await;
    finish_lifecycle(
        per_profile,
        targets.len() > 1,
        output,
        &format!("Unacknowledged case '{case_id}'"),
    )
}

pub async fn run_resolve(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    reason: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Resolving case {case_id}...").dimmed());
    let id = case_id.to_string();
    let reason_owned = reason.map(String::from);
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        let reason = reason_owned.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.resolve(&id, reason.as_deref()).await?)
        }
    })
    .await;
    finish_lifecycle(
        per_profile,
        targets.len() > 1,
        output,
        &format!("Resolved case '{case_id}'"),
    )
}

pub async fn run_close(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Closing case {case_id}...").dimmed());
    let id = case_id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.close(&id).await?)
        }
    })
    .await;
    finish_lifecycle(
        per_profile,
        targets.len() > 1,
        output,
        &format!("Closed case '{case_id}'"),
    )
}

pub async fn run_set_priority(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    priority: &str,
    output: OutputFormat,
) -> Result<()> {
    let normalized = normalize_priority(priority)?;
    eprintln!(
        "{}",
        format!("Overriding priority of case {case_id} -> {normalized}...").dimmed()
    );

    let id = case_id.to_string();
    let prio = normalized.clone();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        let prio = prio.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.set_priority(&id, &prio).await?)
        }
    })
    .await;
    finish_lifecycle(
        per_profile,
        targets.len() > 1,
        output,
        &format!("Set priority of case '{case_id}' to {normalized}"),
    )
}

pub async fn run_clear_priority(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Clearing priority override of case {case_id}...").dimmed()
    );
    let id = case_id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.clear_priority(&id).await?)
        }
    })
    .await;
    finish_lifecycle(
        per_profile,
        targets.len() > 1,
        output,
        &format!("Cleared priority override of case '{case_id}'"),
    )
}

pub async fn run_filter_values(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching case filter values...".dimmed());

    let body = json!({ "filters": {} });
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.filter_values(&body).await?)
        }
    })
    .await;

    render_value_per_profile(per_profile, output, "No filter values returned.")
}

pub async fn run_grouping_keys(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching case grouping keys...".dimmed());

    let per_profile = fan_out(targets, |t| async move {
        let api = CasesApi::new(&t.client);
        Ok(api.grouping_keys().await?)
    })
    .await;

    render_value_per_profile(per_profile, output, "No grouping keys returned.")
}

pub async fn run_events_list(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching events for case {case_id}...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let id = case_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.list_events(&id).await?)
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for mut event in resp.events {
                    if include_profile {
                        if let Value::Object(ref mut m) = event {
                            m.insert("profile".to_string(), Value::String(profile.clone()));
                        }
                    }
                    all_json.push(event);
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
                render::print_no_results("No case events found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_json
                .iter()
                .map(|event| {
                    let profile = event
                        .get("profile")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let event_id = event
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let event_type = event
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let created = event
                        .get("createTime")
                        .or_else(|| event.get("create_time"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    vec![profile, event_id, event_type, created]
                })
                .collect();
            render::render_table(&["Event ID", "Type", "Created"], rows, include_profile);
        }
    }

    Ok(())
}

pub async fn run_event_get(
    targets: &[Arc<ExecutionTarget>],
    event_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching event {event_id}...").dimmed());

    let include_profile = targets.len() > 1;
    let id = event_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.get_event(&id).await?)
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
            render::render_get_text(&all_results, include_profile, "Event not found.", None)?;
        }
    }

    Ok(())
}

pub async fn run_notifications(
    targets: &[Arc<ExecutionTarget>],
    case_ids: &[String],
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!(
            "Fetching notification deliveries for {} case(s)...",
            case_ids.len()
        )
        .dimmed()
    );

    let include_profile = targets.len() > 1;
    let ids = case_ids.to_vec();

    let per_profile = fan_out(targets, |t| {
        let ids = ids.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.list_notification_deliveries(&ids).await?)
        }
    })
    .await;

    // Response shape: { "deliveriesByCase": { "<case-id>": { "notificationDeliveries": [ ... ] } } }
    // For text/json rendering, flatten into rows tagged with caseId so the output is tabular-friendly.
    let mut all_json: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(val) => {
                let map = val.get("deliveriesByCase").and_then(|v| v.as_object());
                if let Some(map) = map {
                    for (case_id, payload) in map {
                        let deliveries = payload
                            .get("notificationDeliveries")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        for mut delivery in deliveries {
                            if let Value::Object(ref mut m) = delivery {
                                m.insert("caseId".to_string(), Value::String(case_id.clone()));
                                if include_profile {
                                    m.insert("profile".to_string(), Value::String(profile.clone()));
                                }
                            }
                            all_json.push(delivery);
                        }
                    }
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
                render::print_no_results("No notification deliveries found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_json
                .iter()
                .map(|d| {
                    let profile = d
                        .get("profile")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let case_id = d
                        .get("caseId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let connector = d
                        .get("connectorType")
                        .or_else(|| d.get("connector_type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string();
                    let status = d
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string();
                    let delivered_at = d
                        .get("deliveredAt")
                        .or_else(|| d.get("delivered_at"))
                        .or_else(|| d.get("createTime"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    vec![profile, case_id, connector, status, delivered_at]
                })
                .collect();
            render::render_table(
                &["Case ID", "Connector", "Status", "Delivered"],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Collect per-profile lifecycle results, emit a success/error line per profile,
/// and render JSON/agents output for the returned payloads.
fn finish_lifecycle(
    per_profile: Vec<(String, Result<Value>)>,
    include_profile: bool,
    output: OutputFormat,
    success_label: &str,
) -> Result<()> {
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(mut v) => {
                if include_profile {
                    render::tag_get_result(&mut v, &profile);
                }
                all_results.push(v);
                eprintln!(
                    "{}",
                    format!("{success_label} in profile '{profile}'.").green()
                );
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
            // Status was already printed above; nothing extra to render in text mode.
        }
    }

    Ok(())
}

fn render_value_per_profile(
    per_profile: Vec<(String, Result<Value>)>,
    output: OutputFormat,
    empty_msg: &str,
) -> Result<()> {
    let include_profile = per_profile.len() > 1;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(mut v) => {
                if include_profile {
                    render::tag_get_result(&mut v, &profile);
                }
                all_results.push(v);
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
            if all_results.is_empty() {
                render::print_no_results(empty_msg);
                return Ok(());
            }
            for val in &all_results {
                if let Some(p) = val.get("_profile").and_then(|v| v.as_str()) {
                    println!("{}", format!("[{p}]").dimmed());
                }
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_priority_accepts_shorthand() {
        assert_eq!(normalize_priority("P1").unwrap(), "CASE_PRIORITY_P1");
        assert_eq!(normalize_priority("p3").unwrap(), "CASE_PRIORITY_P3");
    }

    #[test]
    fn normalize_priority_passes_through_full_enum() {
        assert_eq!(
            normalize_priority("CASE_PRIORITY_P2").unwrap(),
            "CASE_PRIORITY_P2"
        );
    }

    #[test]
    fn normalize_priority_rejects_invalid() {
        assert!(normalize_priority("CRITICAL").is_err());
        assert!(normalize_priority("P9").is_err());
    }

    #[test]
    fn normalize_enum_filter_prefixes_values() {
        let result = normalize_enum_filter(
            &["active".to_string(), "ACKNOWLEDGED".to_string()],
            "CASE_STATUS_",
        );
        assert_eq!(
            result,
            vec![
                "CASE_STATUS_ACTIVE".to_string(),
                "CASE_STATUS_ACKNOWLEDGED".to_string()
            ]
        );
    }

    #[test]
    fn normalize_enum_filter_passes_through_full() {
        let result = normalize_enum_filter(&["CASE_STATUS_ACTIVE".to_string()], "CASE_STATUS_");
        assert_eq!(result, vec!["CASE_STATUS_ACTIVE".to_string()]);
    }
}
