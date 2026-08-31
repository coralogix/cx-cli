pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{Integration, IntegrationsApi};

fn rg_to_json(integration: &Integration, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": integration.id,
        "name": integration.name,
        "type": integration.display_type(),
        "version": integration.display_version(),
        "tags": integration.tags,
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
        eprintln!(
            "{}",
            "Reading integration definition from stdin...".dimmed()
        );
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading integration definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

/// Convert supported CLI input formats into the Integration Service metadata shape.
///
/// `cx integrations get <key>` returns the integration catalog entry together with its
/// registered deployments. Select the requested deployment from that response so users can
/// pass the fetched JSON directly to `update` or `test`.
fn integration_metadata(body: &Value, deployment_id: &str) -> Result<Value> {
    if let Some(metadata) = body.get("metadata") {
        return Ok(metadata.clone());
    }

    if body.get("integrationKey").is_some() {
        let mut metadata = body.clone();
        if let Some(parameters) = metadata.get("parameters").cloned() {
            let Some(object) = metadata.as_object_mut() else {
                unreachable!("a JSON value with integrationKey must be an object");
            };
            object.remove("parameters");
            object.insert(
                "integrationParameters".to_string(),
                json!({ "parameters": parameters }),
            );
        }
        return Ok(metadata);
    }

    let integration_key = body
        .pointer("/integrationDetail/integration/id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "integration JSON must contain `integrationKey`, `metadata`, or \
             `integrationDetail.integration.id`"
            )
        })?;
    let deployment = body
        .pointer("/integrationDetail/default/registered")
        .and_then(Value::as_array)
        .and_then(|deployments| {
            deployments.iter().find(|deployment| {
                deployment.get("id").and_then(Value::as_str) == Some(deployment_id)
            })
        })
        .ok_or_else(|| {
            anyhow::anyhow!("deployment `{deployment_id}` was not found in the integration JSON")
        })?;
    let version = deployment
        .get("definitionVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            anyhow::anyhow!("deployment `{deployment_id}` is missing `definitionVersion`")
        })?;
    let parameters = deployment
        .get("parameters")
        .ok_or_else(|| anyhow::anyhow!("deployment `{deployment_id}` is missing `parameters`"))?;

    Ok(json!({
        "integrationKey": integration_key,
        "integrationParameters": { "parameters": parameters },
        "version": version,
    }))
}

fn test_request(body: &Value, deployment_id: Option<&str>) -> Result<Value> {
    if body.get("integrationData").is_some() && body.get("integrationId").is_some() {
        return Ok(body.clone());
    }

    let deployment_id = deployment_id
        .or_else(|| body.get("integrationId").and_then(Value::as_str))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "`cx integrations test` requires `--id <deployed-integration-id>` unless \
             the JSON contains both `integrationId` and `integrationData`"
            )
        })?;
    let metadata = integration_metadata(body, deployment_id)?;

    Ok(json!({
        "integrationId": deployment_id,
        "integrationData": metadata,
    }))
}

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching integrations...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = IntegrationsApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, Integration)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        // Print the extensions/integrations page link to stderr once per
        // profile. Skip when there are no integrations, since there's
        // nothing to view.
        if !resp.integrations.is_empty() {
            crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                crate::console_url::integrations_url(b)
            })
            .await;
        }
        for entry in resp.integrations {
            let integration = entry.integration;
            let val = rg_to_json(&integration, include_profile, &profile);
            all_json.push(val);
            all_items.push((profile.clone(), integration));
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
                render::print_no_results("No integrations found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, integration)| {
                    vec![
                        profile.clone(),
                        integration.id.clone().unwrap_or_default(),
                        integration.name.clone().unwrap_or_default(),
                        integration.display_type().to_string(),
                        integration.display_version().to_string(),
                        integration.tags.join(", "),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Type", "Version", "Tags"],
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
    eprintln!("{}", format!("Fetching integration {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = IntegrationsApi::new(&t.client);
            Ok(api.get_details(&id).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::integrations_url(b)
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
                "Integration not found.",
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
    eprintln!("{}", "Creating integration...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = IntegrationsApi::new(&t.client);
            Ok(api.save(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(integration) = resp.deployment {
            let name = integration.name.clone().unwrap_or_default();
            render::print_created(
                "Created",
                "integration",
                Some(&name),
                integration.id.as_deref(),
                &profile,
            );
            let val = rg_to_json(&integration, include_profile, &profile);
            crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                crate::console_url::integrations_url(b)
            })
            .await;
            all_results.push(val);
        }
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
    let request = json!({
        "id": id,
        "metadata": integration_metadata(&body, id)?,
    });
    eprintln!("{}", format!("Updating integration {id}...").dimmed());
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let request = request.clone();
        async move {
            let api = IntegrationsApi::new(&t.client);
            Ok(api.update(&request).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Updated integration {id} in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::integrations_url(b)
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
    eprintln!("{}", format!("Deleting integration {id}...").dimmed());
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = IntegrationsApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Integration {id} deleted in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::integrations_url(b)
        })
        .await;
    }
    Ok(())
}

pub async fn run_definition(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching integration definition {id}...").dimmed()
    );
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = IntegrationsApi::new(&t.client);
            Ok(api.get_definition(&id).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::integrations_url(b)
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
                "Integration definition not found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}

pub async fn run_deployed(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching deployed integration {id}...").dimmed()
    );
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = IntegrationsApi::new(&t.client);
            Ok(api.get_deployed(&id).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::integrations_url(b)
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
                "Deployed integration not found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}

pub async fn run_test(
    targets: &[Arc<ExecutionTarget>],
    id: Option<&str>,
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    let request = test_request(&body, id)?;
    eprintln!("{}", "Testing integration...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let request = request.clone();
        async move {
            let api = IntegrationsApi::new(&t.client);
            Ok(api.test(&request).await?)
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
            crate::console_url::integrations_url(b)
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

pub async fn run_template(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching integration template...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = IntegrationsApi::new(&t.client);
        Ok(api.get_template().await?)
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::integrations_url(b)
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
                "Integration template not found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}
