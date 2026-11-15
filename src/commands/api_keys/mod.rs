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
        // Print the API keys page link to stderr once per profile.
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::iam_api_keys_url(b)
        })
        .await;
        for key in resp.keys {
            all_json.push(key_to_json(&key, include_profile, &profile));
            all_items.push((profile.clone(), key));
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
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::iam_api_keys_url(b)
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
        crate::execution::emit_console_link_for_profile(
            targets,
            &profile,
            crate::console_url::iam_api_keys_url,
        )
        .await;
        all_results.push(v);
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
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::iam_api_keys_url(b)
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
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::iam_api_keys_url(b)
        })
        .await;
    }
    Ok(())
}

/// Suffix shared by every data-ingest permission (`logs.data-ingress:SendData`,
/// `rum-ingress:SendData`, ...).
const SEND_DATA_PERMISSION_SUFFIX: &str = ":SendData";

/// All permissions a key grants: its direct permissions plus every permission
/// of its presets.
fn granted_permissions(key: &KeyInfo) -> Vec<&str> {
    let Some(kp) = key.key_permissions.as_ref() else {
        return Vec::new();
    };
    let direct = kp["permissions"].as_array().into_iter().flatten();
    let from_presets = kp["presets"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| p["permissions"].as_array().into_iter().flatten());
    direct
        .chain(from_presets)
        .filter_map(Value::as_str)
        .collect()
}

/// Whether a key from the API-keys list is a Send-Your-Data key: it grants at
/// least one permission, and only data-ingest ones. Covers keys created with
/// the `SendData` and `SendRUMData` presets as well as custom ingest-only keys.
fn is_send_data_key(key: &KeyInfo) -> bool {
    let permissions = granted_permissions(key);
    !permissions.is_empty()
        && permissions
            .iter()
            .all(|p| p.ends_with(SEND_DATA_PERMISSION_SUFFIX))
}

fn preset_names(key: &KeyInfo) -> Vec<&str> {
    key.key_permissions
        .as_ref()
        .and_then(|kp| kp["presets"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|p| p["name"].as_str())
        .collect()
}

/// Merge the legacy send-data endpoint's keys with the Send-Your-Data keys
/// found in the API-keys list. The legacy endpoint only returns keys created
/// through the old send-data flow, so keys created with the `SendData` preset
/// show up only in the list. Keys present in both are reported once.
fn merge_send_data_keys(legacy: &[KeyInfo], listed: &[KeyInfo]) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    legacy
        .iter()
        .chain(listed.iter().filter(|k| is_send_data_key(k)))
        .filter(|k| k.id.as_ref().is_none_or(|id| seen.insert(id.clone())))
        .map(|k| {
            json!({
                "id": k.id,
                "name": k.display_name(),
                "active": k.display_active(),
                "hashed": k.hashed,
                "presets": preset_names(k),
                "value": k.value,
            })
        })
        .collect()
}

pub async fn run_send_data_keys(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching send-data API keys...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = ApiKeysApi::new(&t.client);
        // The two sources need different permissions, so one failing must not
        // hide the keys the other can see. Fail only when both do.
        let (legacy, listed) = tokio::join!(api.get_send_data_keys(), api.list());
        if let (Err(_), Err(e)) = (&legacy, &listed) {
            anyhow::bail!("{e}");
        }
        if let Err(e) = &legacy {
            eprintln!("warning: could not fetch legacy send-data keys: {e}");
        }
        if let Err(e) = &listed {
            eprintln!("warning: could not list API keys: {e}");
        }
        let legacy = legacy.map(|r| r.keys).unwrap_or_default();
        let listed = listed.map(|r| r.keys).unwrap_or_default();
        Ok(Value::Array(merge_send_data_keys(&legacy, &listed)))
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::iam_api_keys_url(b)
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
        // Print the API keys page link to stderr once per profile.
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::iam_api_keys_url(b)
        })
        .await;
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
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
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::iam_api_keys_url(b)
        })
        .await;
    }
    Ok(())
}

/// Body for `POST /api-keys/v3/all/status`: the API takes one `{keyId, active}`
/// entry per key (not a flat `keyIds` + `isActive` pair).
fn admin_status_body(ids: &[String], active: bool) -> Value {
    json!({
        "updates": ids
            .iter()
            .map(|id| json!({ "keyId": id, "active": active }))
            .collect::<Vec<_>>()
    })
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
    let body = admin_status_body(ids, active);

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
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::iam_api_keys_url(b)
        })
        .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str, key_permissions: Value) -> KeyInfo {
        serde_json::from_value(json!({
            "id": id,
            "name": id,
            "active": true,
            "keyPermissions": key_permissions,
        }))
        .unwrap()
    }

    fn preset(name: &str, permissions: &[&str]) -> Value {
        json!({ "name": name, "permissions": permissions })
    }

    #[test]
    fn admin_status_body_uses_per_key_updates() {
        let ids = vec!["k1".to_string(), "k2".to_string()];
        assert_eq!(
            admin_status_body(&ids, false),
            json!({ "updates": [
                { "keyId": "k1", "active": false },
                { "keyId": "k2", "active": false }
            ] })
        );
    }

    #[test]
    fn send_data_preset_keys_are_send_data_keys() {
        let send_data = key(
            "sd",
            json!({
                "presets": [preset("SendData", &["logs.data-ingress:SendData", "spans.data-ingress:SendData"])],
                "permissions": [],
            }),
        );
        let rum = key(
            "rum",
            json!({ "presets": [preset("SendRUMData", &["rum-ingress:SendData"])], "permissions": [] }),
        );
        let custom = key(
            "custom",
            json!({ "presets": [], "permissions": ["metrics.data-ingress:SendData"] }),
        );
        assert!(is_send_data_key(&send_data));
        assert!(is_send_data_key(&rum));
        assert!(is_send_data_key(&custom));
    }

    #[test]
    fn keys_with_non_ingest_permissions_are_not_send_data_keys() {
        let mixed = key(
            "mixed",
            json!({
                "presets": [preset("SendData", &["logs.data-ingress:SendData"])],
                "permissions": ["alerts:ReadConfig"],
            }),
        );
        let query = key(
            "query",
            json!({ "presets": [preset("DataQuerying", &["logs.data-api#high:ReadData"])], "permissions": [] }),
        );
        let empty = key("empty", json!({ "presets": [], "permissions": [] }));
        let unknown: KeyInfo = serde_json::from_value(json!({ "id": "unknown" })).unwrap();
        assert!(!is_send_data_key(&mixed));
        assert!(!is_send_data_key(&query));
        assert!(!is_send_data_key(&empty));
        assert!(!is_send_data_key(&unknown));
    }

    #[test]
    fn merge_keeps_legacy_keys_adds_listed_ones_and_dedupes() {
        let legacy = vec![key("legacy", Value::Null), key("both", Value::Null)];
        let listed = vec![
            key(
                "both",
                json!({ "presets": [preset("SendData", &["logs.data-ingress:SendData"])], "permissions": [] }),
            ),
            key(
                "new",
                json!({ "presets": [preset("SendData", &["logs.data-ingress:SendData"])], "permissions": [] }),
            ),
            key(
                "admin",
                json!({ "presets": [preset("TeamAdmin", &["team-members:Manage"])], "permissions": [] }),
            ),
        ];
        let merged = merge_send_data_keys(&legacy, &listed);
        let ids: Vec<&str> = merged.iter().map(|k| k["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["legacy", "both", "new"]);
        assert_eq!(merged[2]["presets"], json!(["SendData"]));
    }
}
