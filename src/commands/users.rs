use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::api::users::{User, UsersApi};
use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;

fn user_to_json(user: &User, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "user_id": user.user_id,
        "user_account_id": user.user_account_id,
        "name": user.display_name(),
        "email": user.display_username(),
        "status": user.display_status(),
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading user definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading user definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

pub async fn run_search(
    targets: &[Arc<ExecutionTarget>],
    query: Option<&str>,
    status: Option<&str>,
    page_size: Option<&str>,
    page_token: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Searching users...".dimmed());
    let include_profile = targets.len() > 1;

    let query = query.map(String::from);
    let status = status.map(String::from);
    let page_size = page_size.map(String::from);
    let page_token = page_token.map(String::from);

    let per_profile = fan_out(targets, |t| {
        let query = query.clone();
        let status = status.clone();
        let page_size = page_size.clone();
        let page_token = page_token.clone();
        async move {
            let api = UsersApi::new(&t.client);
            let team_id = "";
            let mut params: Vec<(&str, String)> = Vec::new();
            if let Some(ref q) = query {
                params.push(("query", q.clone()));
            }
            if let Some(ref s) = status {
                params.push(("status", s.clone()));
            }
            if let Some(ref ps) = page_size {
                params.push(("pageSize", ps.clone()));
            }
            if let Some(ref pt) = page_token {
                params.push(("pageToken", pt.clone()));
            }
            let params_refs: Vec<(&str, &str)> =
                params.iter().map(|(k, v)| (*k, v.as_str())).collect();
            if params_refs.is_empty() {
                Ok(api.search(team_id).await?)
            } else {
                Ok(api.search_with_params(team_id, &params_refs).await?)
            }
        }
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, User)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for user in resp.users {
                    all_json.push(user_to_json(&user, include_profile, &profile));
                    all_items.push((profile.clone(), user));
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
                render::print_no_results("No users found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, user)| {
                    vec![
                        profile.clone(),
                        user.user_id.clone().unwrap_or_default(),
                        user.display_name(),
                        user.display_username().to_string(),
                        String::new(), // Role not available in search response
                        user.display_status().to_string(),
                    ]
                })
                .collect();
            render::render_table(
                &["User ID", "Name", "Email", "Role", "Status"],
                rows,
                include_profile,
            );
        }
    }
    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    user_id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching user {user_id}...").dimmed());
    let include_profile = targets.len() > 1;
    let user_id = user_id.to_string();

    let per_profile = fan_out(targets, |t| {
        let user_id = user_id.clone();
        async move {
            let api = UsersApi::new(&t.client);
            let team_id = "";
            Ok(api.get(team_id, &user_id).await?)
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
                "User not found.",
                None::<&dyn Fn(&Value)>,
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
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Creating user(s)...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = UsersApi::new(&t.client);
            let team_id = "";
            api.create(team_id, &body).await?;
            Ok(())
        }
    })
    .await;

    for (profile, result) in per_profile {
        match result {
            Ok(()) => {
                eprintln!(
                    "{}",
                    format!("Created user(s) in profile '{profile}'.").green()
                );
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json | OutputFormat::Agents | OutputFormat::Text => {}
    }
    Ok(())
}

pub async fn run_update(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Updating user(s)...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = UsersApi::new(&t.client);
            let team_id = "";
            Ok(api.update(team_id, &body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                eprintln!(
                    "{}",
                    format!(
                        "Updated {} user(s) in profile '{profile}'.",
                        resp.user_account_ids.len()
                    )
                    .green()
                );
                let mut v = json!({ "user_account_ids": resp.user_account_ids });
                if targets.len() > 1 {
                    if let Value::Object(ref mut m) = v {
                        m.insert("profile".to_string(), Value::String(profile.to_string()));
                    }
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
        OutputFormat::Text => {}
    }
    Ok(())
}

pub async fn run_set_status(
    targets: &[Arc<ExecutionTarget>],
    user_ids: &[String],
    status: &str,
) -> Result<()> {
    eprintln!(
        "{}",
        format!(
            "Setting status '{}' for {} user(s)...",
            status,
            user_ids.len()
        )
        .dimmed()
    );
    let body = json!({ "userAccountIds": user_ids, "status": status });

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = UsersApi::new(&t.client);
            let team_id = "";
            api.update_statuses(team_id, &body).await?;
            Ok(())
        }
    })
    .await;
    for (profile, result) in per_profile {
        match result {
            Ok(()) => eprintln!(
                "{}",
                format!("Updated user status in profile '{profile}'.").green()
            ),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }
    Ok(())
}
