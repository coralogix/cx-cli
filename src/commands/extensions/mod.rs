pub mod api;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{DeployedExtension, Extension, ExtensionsApi};

fn extension_to_json(ext: &Extension, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": ext.id,
        "name": ext.name,
        "version": ext.version,
        "deployed": ext.deployed,
        "updated": ext.updated,
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
        eprintln!("{}", "Reading extension definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading extension definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching extensions...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = ExtensionsApi::new(&t.client);
        Ok(api.list_all().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, Extension)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        // Print the extensions/integrations page link to stderr once per
        // profile. Skip when there are no extensions, since there's
        // nothing to view.
        if !resp.extensions.is_empty() {
            crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                crate::console_url::integrations_url(b)
            })
            .await;
        }
        for ext in resp.extensions {
            let val = extension_to_json(&ext, include_profile, &profile);
            all_json.push(val);
            all_items.push((profile.clone(), ext));
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
                render::print_no_results("No extensions found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, ext)| {
                    vec![
                        profile.clone(),
                        ext.id.clone().unwrap_or_default(),
                        ext.name.clone().unwrap_or_default(),
                        ext.version.clone().unwrap_or_default(),
                        render::bool_display(ext.deployed),
                        ext.updated.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Version", "Deployed", "Updated"],
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
    eprintln!("{}", format!("Fetching extension {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = ExtensionsApi::new(&t.client);
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
                "Extension not found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}

fn deployed_extension_to_json(
    ext: &DeployedExtension,
    name: Option<&str>,
    include_profile: bool,
    profile: &str,
) -> Value {
    let mut v = json!({
        "id": ext.id,
        "name": name,
        "version": ext.version,
        "applications": ext.applications,
        "subsystems": ext.subsystems,
        "deployed_items": ext.deployed_item_count(),
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

fn list_display(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(", ")
    }
}

pub async fn run_deployed(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching deployed extensions...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = ExtensionsApi::new(&t.client);
        let deployed = api.list_deployed().await?;
        // The deployed endpoint returns ids but no human-readable names;
        // resolve them from the catalog, best-effort. A catalog failure
        // must not fail the command.
        let names: HashMap<String, String> = match api.list_all().await {
            Ok(catalog) => catalog
                .extensions
                .into_iter()
                .filter_map(|e| Some((e.id?, e.name?)))
                .collect(),
            Err(_) => HashMap::new(),
        };
        Ok((deployed, names))
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, DeployedExtension, Option<String>)> = Vec::new();
    for (profile, (resp, names)) in report_errors_and_collect_successes(per_profile)? {
        // Print the extensions/integrations page link to stderr once per
        // profile. Skip when there are no extensions, since there's
        // nothing to view.
        if !resp.deployed_extensions.is_empty() {
            crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                crate::console_url::integrations_url(b)
            })
            .await;
        }
        for ext in resp.deployed_extensions {
            let name = ext.id.as_ref().and_then(|id| names.get(id)).cloned();
            let val = deployed_extension_to_json(&ext, name.as_deref(), include_profile, &profile);
            all_json.push(val);
            all_items.push((profile.clone(), ext, name));
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
                render::print_no_results("No deployed extensions found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, ext, name)| {
                    vec![
                        profile.clone(),
                        ext.id.clone().unwrap_or_default(),
                        name.clone().unwrap_or_else(|| "-".to_string()),
                        ext.version.clone().unwrap_or_default(),
                        list_display(&ext.applications),
                        list_display(&ext.subsystems),
                        ext.deployed_item_count()
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                    ]
                })
                .collect();
            render::render_table(
                &[
                    "ID",
                    "Name",
                    "Version",
                    "Applications",
                    "Subsystems",
                    "Items",
                ],
                rows,
                include_profile,
            );
        }
    }
    Ok(())
}

pub async fn run_deploy(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Deploying extension...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ExtensionsApi::new(&t.client);
            Ok(api.deploy(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Extension deployed in profile '{profile}'.").green()
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

pub async fn run_update(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Updating extension...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ExtensionsApi::new(&t.client);
            Ok(api.update(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Extension updated in profile '{profile}'.").green()
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

pub async fn run_undeploy(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Undeploying extension...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ExtensionsApi::new(&t.client);
            Ok(api.undeploy(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Extension undeployed in profile '{profile}'.").green()
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
