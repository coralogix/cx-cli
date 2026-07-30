pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{ApiKeysApi, KeyInfo};

fn key_to_json(key: &KeyInfo, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": key.id,
        "name": key.display_name(),
        "owner": key.owner,
        "active": key.display_active(),
        "hashed": key.hashed,
        "value": key.value,
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
        eprintln!("{}", "Reading API key definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading API key definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

/// Print the "View in Coralogix" link for the IAM API Keys settings page,
/// if a console base URL can be resolved for `profile`.
///
/// API keys are managed from a flat settings page (`#/settings/api-keys`) -
/// create/edit is a dialog with no id reflected in the URL - so every
/// mutation links to that same page.
async fn print_api_keys_console_link(targets: &[Arc<ExecutionTarget>], profile: &str) {
    if let Some(target) = crate::execution::find_target(targets, profile) {
        if let Some(base) = target.console_base().await {
            render::print_console_link(&crate::console_url::iam_api_keys_url(&base));
        }
    }
}

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching API keys...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = ApiKeysApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, KeyInfo)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        print_api_keys_console_link(targets, &profile).await;
        for key in resp.keys {
            all_json.push(key_to_json(&key, include_profile, &profile));
            all_items.push((profile.clone(), key));
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
                render::print_no_results("No API keys found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, key)| {
                    let owner = key
                        .owner
                        .as_ref()
                        .map(|o| serde_json::to_string(o).unwrap_or_default())
                        .unwrap_or_default();
                    vec![
                        profile.clone(),
                        key.id.clone().unwrap_or_default(),
                        key.display_name().to_string(),
                        owner,
                        key.display_active().to_string(),
                        String::new(),
                        key.hashed.map(|h| h.to_string()).unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Owner", "Active", "Created", "Hashed Key"],
                rows,
                include_profile,
            );
        }
    }
    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching API key {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = ApiKeysApi::new(&t.client);
            Ok(api.get(&id).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        print_api_keys_console_link(targets, &profile).await;
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
            render::render_get_text(
                &all_results,
                include_profile,
                "API key not found.",
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
    eprintln!("{}", "Creating API key...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ApiKeysApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        render::print_created(
            "Created",
            "API key",
            resp.name.as_deref(),
            resp.key_id.as_deref(),
            &profile,
        );
        print_api_keys_console_link(targets, &profile).await;
        let mut v = json!({
            "key_id": resp.key_id,
            "name": resp.name,
            "value": resp.value,
        });
        if targets.len() > 1 {
            if let Value::Object(ref mut m) = v {
                m.insert("profile".to_string(), Value::String(profile.to_string()));
            }
        }
        all_results.push(v);
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
    id: &str,
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", format!("Updating API key {id}...").dimmed());
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        let id = id.clone();
        async move {
            let api = ApiKeysApi::new(&t.client);
            Ok(api.update(&id, &body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Updated API key in profile '{profile}'.").green()
        );
        print_api_keys_console_link(targets, &profile).await;
        all_results.push(val);
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

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    eprintln!("{}", format!("Deleting API key {id}...").dimmed());
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = ApiKeysApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("API key {id} deleted in profile '{profile}'.").green()
        );
        print_api_keys_console_link(targets, &profile).await;
    }
    Ok(())
}

pub async fn run_send_data_keys(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching send-data API keys...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = ApiKeysApi::new(&t.client);
        let resp = api.get_send_data_keys().await?;
        let keys_json: Vec<Value> = resp
            .keys
            .iter()
            .map(|k| {
                json!({
                    "id": k.id,
                    "name": k.display_name(),
                    "active": k.display_active(),
                    "hashed": k.hashed,
                    "value": k.value,
                })
            })
            .collect();
        Ok(Value::Array(keys_json))
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        print_api_keys_console_link(targets, &profile).await;
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
            render::render_get_text(
                &all_results,
                include_profile,
                "No send-data keys found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}

pub async fn run_admin_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching all team members' API keys...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = ApiKeysApi::new(&t.client);
        let resp = api.get_team_members_keys().await?;
        let keys_json: Vec<Value> = resp
            .keys
            .iter()
            .map(|k| {
                json!({
                    "api_key": k.api_key.as_ref().map(|ak| json!({
                        "id": ak.id,
                        "key_name": ak.display_name(),
                        "is_active": ak.display_active(),
                    })),
                    "permissions": k.permissions,
                    "presets": k.presets,
                })
            })
            .collect();
        Ok(Value::Array(keys_json))
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        print_api_keys_console_link(targets, &profile).await;
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
            render::render_get_text(
                &all_results,
                include_profile,
                "No team API keys found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}

pub async fn run_admin_delete(targets: &[Arc<ExecutionTarget>], ids: &[String]) -> Result<()> {
    eprintln!(
        "{}",
        format!("Bulk deleting {} API key(s)...", ids.len()).dimmed()
    );
    let body = json!({ "keyIds": ids });

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ApiKeysApi::new(&t.client);
            api.bulk_delete(&body).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Bulk deleted API keys in profile '{profile}'.").green()
        );
        print_api_keys_console_link(targets, &profile).await;
    }
    Ok(())
}

pub async fn run_admin_set_status(
    targets: &[Arc<ExecutionTarget>],
    ids: &[String],
    active: bool,
) -> Result<()> {
    let action = if active { "Activating" } else { "Deactivating" };
    eprintln!(
        "{}",
        format!("{action} {} API key(s)...", ids.len()).dimmed()
    );
    let body = json!({ "keyIds": ids, "isActive": active });

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ApiKeysApi::new(&t.client);
            api.update_status(&body).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Updated API key status in profile '{profile}'.").green()
        );
        print_api_keys_console_link(targets, &profile).await;
    }
    Ok(())
}
