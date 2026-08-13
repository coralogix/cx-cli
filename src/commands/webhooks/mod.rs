pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{Webhook, WebhooksApi};

fn webhook_to_json(webhook: &Webhook, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": webhook.id,
        "name": webhook.name,
        "type": webhook.display_type(),
        "url": webhook.url,
        "created_at": webhook.created_at,
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

/// Extract a webhook ID from a create response, trying the shapes the API
/// has been observed to return: nested `webhook.id`, or a bare top-level
/// `id`. IDs may come back as a JSON string or number. Returns `None` when
/// the response carried no ID so callers can flag that rather than
/// fabricate one.
fn webhook_id_from_response(resp: &Value) -> Option<String> {
    fn at(v: &Value, pointer: &str) -> Option<String> {
        match v.pointer(pointer)? {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }
    at(resp, "/webhook/id").or_else(|| at(resp, "/id"))
}

/// Pull the display name out of a create *request* body. The create response
/// only carries an id, so the name has to come from what was sent. Outgoing
/// webhook payloads nest it under `data` (`{"data": {"name": ...}}`); the
/// root `name` fallback covers hand-written flat payloads.
fn webhook_name_from_request(body: &Value) -> Option<&str> {
    body.pointer("/data/name")
        .or_else(|| body.get("name"))
        .and_then(|v| v.as_str())
}

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading webhook definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading webhook definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching webhooks...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = WebhooksApi::new(&t.client);
        Ok(api.list_all().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, Webhook)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        // Print the outbound-webhooks page link to stderr once per profile.
        // Skip when there are no webhooks, since there's nothing to view.
        if !resp.deployed.is_empty() {
            crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                crate::console_url::webhooks_url(b)
            })
            .await;
        }
        for webhook in resp.deployed {
            let webhook_json = webhook_to_json(&webhook, include_profile, &profile);
            all_json.push(webhook_json);
            all_items.push((profile.clone(), webhook));
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
                render::print_no_results("No webhooks found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, webhook)| {
                    vec![
                        profile.clone(),
                        webhook.id.clone().unwrap_or_default(),
                        webhook.name.clone().unwrap_or_default(),
                        webhook.display_type().to_string(),
                        webhook.url.clone().unwrap_or_default(),
                        webhook.created_at.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Type", "URL", "Created At"],
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
    eprintln!("{}", format!("Fetching webhook {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = WebhooksApi::new(&t.client);
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
            crate::console_url::webhooks_url(b)
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
                "Webhook not found.",
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
    let name = webhook_name_from_request(&body)
        .unwrap_or("<unnamed>")
        .to_string();
    eprintln!("{}", "Creating webhook...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = WebhooksApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut resp) in report_errors_and_collect_successes(per_profile)? {
        let created_id = webhook_id_from_response(&resp);
        render::print_created(
            "Created",
            "webhook",
            Some(&name),
            created_id.as_deref(),
            &profile,
        );
        if include_profile {
            render::tag_get_result(&mut resp, &profile);
        }
        crate::execution::emit_console_link_for_profile(
            targets,
            &profile,
            crate::console_url::webhooks_url,
        )
        .await;
        all_results.push(resp);
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
    eprintln!("{}", format!("Updating webhook {id}...").dimmed());
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        let id = id.clone();
        async move {
            let api = WebhooksApi::new(&t.client);
            Ok(api.update(&id, &body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Updated webhook {id} in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::webhooks_url(b)
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
    eprintln!("{}", format!("Deleting webhook {id}...").dimmed());
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = WebhooksApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Webhook {id} deleted in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::webhooks_url(b)
        })
        .await;
    }
    Ok(())
}

pub async fn run_test(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Testing webhook {id}...").dimmed());
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = WebhooksApi::new(&t.client);
            Ok(api.test(&id).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Test completed in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::webhooks_url(b)
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
            for val in &all_results {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }
    Ok(())
}

pub async fn run_types(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching webhook types...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = WebhooksApi::new(&t.client);
        Ok(api.list_types().await?)
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::webhooks_url(b)
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
            for val in &all_results {
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
    fn webhook_id_from_response_reads_wrapped_shape() {
        let resp = json!({ "webhook": { "id": "wh-001", "name": "Slack Notify" } });
        assert_eq!(webhook_id_from_response(&resp).as_deref(), Some("wh-001"));
    }

    #[test]
    fn webhook_id_from_response_reads_bare_shape() {
        // What the live API actually returns: the created webhook directly,
        // with no `webhook` envelope.
        let resp = json!({ "id": "wh-002", "name": "Slack Notify" });
        assert_eq!(webhook_id_from_response(&resp).as_deref(), Some("wh-002"));
    }

    #[test]
    fn webhook_id_from_response_reads_numeric_id() {
        let resp = json!({ "webhook": { "id": 7 } });
        assert_eq!(webhook_id_from_response(&resp).as_deref(), Some("7"));
    }

    #[test]
    fn webhook_id_from_response_none_when_id_absent() {
        let resp = json!({});
        assert_eq!(webhook_id_from_response(&resp), None);
    }

    #[test]
    fn webhook_name_from_request_reads_nested_data_name() {
        // The real outgoing-webhook create payload shape.
        let body = json!({ "data": { "name": "Slack Notify", "type": "slack" } });
        assert_eq!(webhook_name_from_request(&body), Some("Slack Notify"));
    }

    #[test]
    fn webhook_name_from_request_falls_back_to_root_name() {
        let body = json!({ "name": "Flat Payload" });
        assert_eq!(webhook_name_from_request(&body), Some("Flat Payload"));
    }

    #[test]
    fn webhook_name_from_request_prefers_data_name_over_root() {
        let body = json!({ "name": "outer", "data": { "name": "inner" } });
        assert_eq!(webhook_name_from_request(&body), Some("inner"));
    }

    #[test]
    fn webhook_name_from_request_none_when_absent() {
        assert_eq!(webhook_name_from_request(&json!({})), None);
        // A non-string name is not a usable display name either.
        assert_eq!(webhook_name_from_request(&json!({ "name": 7 })), None);
    }
}
