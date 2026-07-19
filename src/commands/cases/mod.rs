pub mod api;

use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use colored::Colorize;
use serde_json::Value;
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

/// Recursively rewrite any string value that matches a known user ID to that
/// user's email. Events carry user IDs in assorted shapes (`actor.user.userId`,
/// `assigneeUserId`, `previousAssigneeUserId`, ...); resolving by value lets us
/// surface emails everywhere they appear without special-casing each field.
/// IDs absent from the directory (e.g. service accounts) are left untouched.
fn substitute_user_emails(value: &mut Value, directory: &TeammateDirectory) {
    match value {
        Value::Object(map) => {
            for v in map.values_mut() {
                substitute_user_emails(v, directory);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                substitute_user_emails(v, directory);
            }
        }
        Value::String(s) => {
            if let Some(email) = directory.email_for(s) {
                *s = email.to_string();
            }
        }
        _ => {}
    }
}

/// Strip the noisy `CASE_STATUS_` / `CASE_PRIORITY_` / `CASE_CATEGORY_` prefix
/// from an enum-like string so the table shows `ACTIVE` instead of
/// `CASE_STATUS_ACTIVE`.
fn humanize_enum(s: &str) -> String {
    for prefix in ["CASE_STATUS_", "CASE_PRIORITY_", "CASE_CATEGORY_"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    s.to_string()
}

/// Render an event's `actor` into a readable string: a teammate email (or raw
/// user ID) for human actors, or "system" for automated events.
fn format_event_actor(event: &Value) -> String {
    let Some(actor) = event.get("actor").and_then(|v| v.as_object()) else {
        return "-".to_string();
    };
    if let Some(user) = actor.get("user").and_then(|v| v.as_object()) {
        return user
            .get("email")
            .or_else(|| user.get("userId"))
            .or_else(|| user.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("user")
            .to_string();
    }
    if actor.contains_key("system") {
        return "system".to_string();
    }
    // Unknown actor shape: surface its variant key rather than a blank cell.
    actor
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| "-".to_string())
}

/// Summarize the change an event represents from its `eventData` payload, e.g.
/// `PENDING_ACTIVATION → ACTIVE` for a status change or `P1` for case creation.
fn format_event_details(event: &Value) -> String {
    let Some((_kind, payload)) = event
        .get("eventData")
        .and_then(|v| v.as_object())
        .and_then(|m| m.iter().next())
    else {
        return "-".to_string();
    };
    let Some(obj) = payload.as_object() else {
        return "-".to_string();
    };
    let get = |k: &str| obj.get(k).and_then(|v| v.as_str()).map(humanize_enum);

    // Common "before → after" transitions (status, priority, category, ...).
    if let (Some(old), Some(new)) = (get("oldStatus"), get("newStatus")) {
        return format!("{old} → {new}");
    }
    if let (Some(old), Some(new)) = (get("oldPriority"), get("newPriority")) {
        return format!("{old} → {new}");
    }
    if let Some(priority) = get("priority") {
        return priority;
    }
    if let Some(status) = get("status") {
        return status;
    }

    // Generic fallback: compact "key: value" pairs of string fields so the row
    // is never empty for event shapes we don't model explicitly.
    let pairs: Vec<String> = obj
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| format!("{k}: {}", humanize_enum(s))))
        .collect();
    if pairs.is_empty() {
        "-".to_string()
    } else {
        pairs.join(", ")
    }
}

/// A notification delivery carries its outcome as a oneof-style variant key
/// (e.g. `noRouterMatched`, plus connector success/failure variants), alongside
/// the bookkeeping fields below. Surface that variant as a readable status.
fn format_delivery_status(delivery: &Value) -> String {
    const BOOKKEEPING: [&str; 4] = ["timestamp", "requestNotificationId", "caseId", "profile"];
    let Some(obj) = delivery.as_object() else {
        return "-".to_string();
    };
    obj.keys()
        .find(|k| !BOOKKEEPING.contains(&k.as_str()))
        .map(|k| humanize_camel_case(k))
        .unwrap_or_else(|| "-".to_string())
}

/// Turn a camelCase identifier into space-separated words with a leading
/// capital, e.g. `noRouterMatched` → `No router matched`.
fn humanize_camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i != 0 {
            out.push(' ');
            out.extend(ch.to_lowercase());
        } else if i == 0 {
            out.extend(ch.to_uppercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// One rendered table row for a notification delivery. A single delivery fans
/// out to one row per connector attempt (`attempted.attempts[]`); deliveries
/// with no attempts (e.g. `noRouterMatched`) produce a single summary row.
struct DeliveryRow {
    profile: String,
    case_id: String,
    connector: String,
    connector_type: String,
    status: String,
    evidence_url: String,
    timestamp: String,
}

/// Flatten one delivery payload into its per-attempt rows.
fn delivery_rows(delivery: &Value) -> Vec<DeliveryRow> {
    let profile = str_field(delivery, "profile");
    let case_id = str_field(delivery, "caseId");
    let timestamp = str_field(delivery, "timestamp");

    let attempts = delivery
        .get("attempted")
        .and_then(|a| a.get("attempts"))
        .and_then(|v| v.as_array());

    let Some(attempts) = attempts.filter(|a| !a.is_empty()) else {
        // No connector attempts (e.g. noRouterMatched): one summary row whose
        // status is the top-level outcome variant.
        return vec![DeliveryRow {
            profile,
            case_id,
            connector: "-".to_string(),
            connector_type: "-".to_string(),
            status: format_delivery_status(delivery),
            evidence_url: "-".to_string(),
            timestamp,
        }];
    };

    attempts
        .iter()
        .map(|attempt| {
            let connector = attempt
                .get("connector")
                .map(|c| str_field(c, "connectorName"))
                .filter(|s| s != "-")
                .unwrap_or_else(|| "-".to_string());
            let connector_type = attempt
                .get("connector")
                .and_then(|c| c.get("connectorType"))
                .and_then(|v| v.as_str())
                .map(|t| t.strip_prefix("CONNECTOR_TYPE_").unwrap_or(t).to_string())
                .unwrap_or_else(|| "-".to_string());
            // outcome is a oneof: success | failure (+ maybe more). Its variant
            // key is the status; evidenceUrl (when present) lives inside it.
            let outcome = attempt.get("outcome").and_then(|v| v.as_object());
            let status = outcome
                .and_then(|o| o.keys().next())
                .map(|k| humanize_camel_case(k))
                .unwrap_or_else(|| "-".to_string());
            let evidence_url = outcome
                .and_then(|o| o.values().next())
                .map(|v| str_field(v, "evidenceUrl"))
                .filter(|s| s != "-")
                .unwrap_or_else(|| "-".to_string());
            DeliveryRow {
                profile: profile.clone(),
                case_id: case_id.clone(),
                connector,
                connector_type,
                status,
                evidence_url,
                timestamp: timestamp.clone(),
            }
        })
        .collect()
}

/// Read a string field, returning "-" when missing — the table's empty-cell
/// convention.
fn str_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("-")
        .to_string()
}

// ── Subcommand runners ────────────────────────────────────────────────────────

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
    let target_count = per_profile.len();
    let mut error_count = 0usize;
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
            Err(e) => {
                error_count += 1;
                eprintln!("{}", format!("error from profile '{profile}': {e:#}").red());
            }
        }
    }
    if target_count > 0 && error_count == target_count {
        bail!("all profiles returned errors; see above for details");
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

pub async fn run_comment(
    targets: &[Arc<ExecutionTarget>],
    case_id: &str,
    text: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Adding comment to case {case_id}...").dimmed()
    );

    let id = case_id.to_string();
    let text = text.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        let text = text.clone();
        async move {
            let api = CasesApi::new(&t.client);
            Ok(api.create_comment(&id, &text).await?)
        }
    })
    .await;

    finish_lifecycle(
        per_profile,
        targets.len() > 1,
        output,
        &format!("Added comment to case '{case_id}'"),
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
            let (user_id, resolved_directory) = if input.contains('@') {
                let directory = api.teammate_directory().await.map_err(|e| {
                    anyhow!(
                        "failed to fetch team members to resolve email '{input}': {e:#}\n\
                         hint: resolving an email requires read access to the team \
                         users directory (GET /mgmt/openapi/5/aaa/teams/v2/{{team_id}}/search). \
                         If your API key lacks that scope, pass the user ID directly: \
                         --user <user-id>."
                    )
                })?;
                let uid = directory.resolve_to_user_id(&input)?;
                (uid, Some(directory))
            } else {
                (input.clone(), None)
            };
            let mut result = api.assign(&id, &user_id).await?;
            // Surface the email in the response so the user never sees the userId.
            // Reuse the directory already fetched on the email path; only the
            // raw-user-ID path needs a fresh (best-effort) fetch here.
            let directory = match resolved_directory {
                Some(d) => d,
                None => api.teammate_directory().await.unwrap_or_default(),
            };
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
            // Fetch events and the team directory together; the directory is
            // best-effort so events still render if the lookup is unavailable.
            let (events_res, dir_res) =
                tokio::join!(api.list_events(&id), api.teammate_directory());
            let events = events_res?;
            let directory = dir_res.unwrap_or_default();
            Ok::<_, anyhow::Error>((events, directory))
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let target_count = per_profile.len();
    let mut error_count = 0usize;
    for (profile, result) in per_profile {
        match result {
            Ok((resp, directory)) => {
                for mut event in resp.events {
                    substitute_user_emails(&mut event, &directory);
                    if include_profile {
                        if let Value::Object(ref mut m) = event {
                            m.insert("profile".to_string(), Value::String(profile.clone()));
                        }
                    }
                    all_json.push(event);
                }
            }
            Err(e) => {
                error_count += 1;
                eprintln!("{}", format!("error from profile '{profile}': {e:#}").red());
            }
        }
    }
    if target_count > 0 && error_count == target_count {
        bail!("all profiles returned errors; see above for details");
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
                        .get("eventId")
                        .or_else(|| event.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let event_type = event
                        .get("eventData")
                        .and_then(|v| v.as_object())
                        .and_then(|m| m.keys().next())
                        .map(|k| k.to_string())
                        .unwrap_or_default();
                    let created = event
                        .get("createTime")
                        .or_else(|| event.get("create_time"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let actor = format_event_actor(event);
                    let details = format_event_details(event);
                    vec![profile, event_id, event_type, actor, details, created]
                })
                .collect();
            render::render_table(
                &["Event ID", "Type", "Actor", "Details", "Timestamp"],
                rows,
                include_profile,
            );
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
            let (event_res, dir_res) = tokio::join!(api.get_event(&id), api.teammate_directory());
            let mut event = event_res?;
            let directory = dir_res.unwrap_or_default();
            substitute_user_emails(&mut event, &directory);
            Ok::<_, anyhow::Error>(event)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    let target_count = per_profile.len();
    let mut error_count = 0usize;
    for (profile, result) in per_profile {
        match result {
            Ok(mut val) => {
                if include_profile {
                    render::tag_get_result(&mut val, &profile);
                }
                all_results.push(val);
            }
            Err(e) => {
                error_count += 1;
                eprintln!("{}", format!("error from profile '{profile}': {e:#}").red());
            }
        }
    }
    if target_count > 0 && error_count == target_count {
        bail!("all profiles returned errors; see above for details");
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
            // The deliveries endpoint accepts both readable IDs (e.g.
            // "CASE-764019") and UUIDs directly, so the IDs the user passed go
            // straight through and come back keyed under the same values.
            Ok(api.list_notification_deliveries(&ids).await?)
        }
    })
    .await;

    // Response shape: { "deliveriesByCase": { "<case-id>": { "notificationDeliveries": [ ... ] } } }
    // For text/json rendering, flatten into rows tagged with caseId so the output is tabular-friendly.
    let mut all_json: Vec<Value> = Vec::new();
    let target_count = per_profile.len();
    let mut error_count = 0usize;
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
            Err(e) => {
                error_count += 1;
                eprintln!("{}", format!("error from profile '{profile}': {e:#}").red());
            }
        }
    }
    if target_count > 0 && error_count == target_count {
        bail!("all profiles returned errors; see above for details");
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
            // The Case ID column is redundant when the user queried a single
            // case — every row repeats the same value.
            let include_case_id = case_ids.len() > 1;
            let rows: Vec<Vec<String>> = all_json
                .iter()
                .flat_map(delivery_rows)
                .map(|r| {
                    let mut row = Vec::with_capacity(6);
                    row.push(r.profile);
                    if include_case_id {
                        row.push(r.case_id);
                    }
                    row.push(r.connector);
                    row.push(r.connector_type);
                    row.push(r.status);
                    row.push(r.evidence_url);
                    row.push(r.timestamp);
                    row
                })
                .collect();
            let headers: &[&str] = if include_case_id {
                &[
                    "Case ID",
                    "Connector",
                    "Type",
                    "Status",
                    "Evidence URL",
                    "Timestamp",
                ]
            } else {
                &["Connector", "Type", "Status", "Evidence URL", "Timestamp"]
            };
            render::render_table(headers, rows, include_profile);
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
    let target_count = per_profile.len();
    let mut error_count = 0usize;
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
            Err(e) => {
                error_count += 1;
                eprintln!("{}", format!("error from profile '{profile}': {e:#}").red());
            }
        }
    }
    if target_count > 0 && error_count == target_count {
        bail!("all profiles returned errors; see above for details");
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

    fn directory() -> TeammateDirectory {
        TeammateDirectory::from_users(vec![api::TeamUser {
            user_id: Some("uid-known".into()),
            username: Some("alice@example.com".into()),
        }])
    }

    #[test]
    fn substitute_user_emails_maps_known_ids_in_event() {
        // An "assigned" event: actor + assigneeUserId both reference users.
        let mut event = serde_json::json!({
            "eventId": "e1",
            "actor": { "user": { "userId": "uid-known" } },
            "eventData": {
                "assigned": {
                    "assigneeUserId": "uid-known",
                    "previousAssigneeUserId": "uid-unknown"
                }
            }
        });
        substitute_user_emails(&mut event, &directory());

        assert_eq!(
            event["actor"]["user"]["userId"], "alice@example.com",
            "actor user ID should resolve to email"
        );
        assert_eq!(
            event["eventData"]["assigned"]["assigneeUserId"],
            "alice@example.com"
        );
        // Unknown IDs (e.g. service accounts) are left untouched.
        assert_eq!(
            event["eventData"]["assigned"]["previousAssigneeUserId"],
            "uid-unknown"
        );
    }

    #[test]
    fn substitute_user_emails_is_a_noop_without_matches() {
        let mut event = serde_json::json!({
            "eventData": { "statusChanged": { "oldStatus": "ACTIVE", "newStatus": "RESOLVED" } }
        });
        let before = event.clone();
        substitute_user_emails(&mut event, &directory());
        assert_eq!(event, before);
    }
}
