pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{TcoPoliciesApi, TcoPolicy};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn policy_to_json(policy: &TcoPolicy, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": policy.id,
        "name": policy.display_name(),
        "priority": policy.display_priority(),
        "source_type": policy.display_source_type(),
        "severity": policy.display_severity(),
        "archive_retention": policy.display_archive_retention(),
        "enabled": policy.enabled,
        "created_at": policy.created_at,
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
        eprintln!("{}", "Reading TCO policy definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading TCO policy definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching TCO policies...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = TcoPoliciesApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, TcoPolicy)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        // One static TCO policies page link per profile, not per policy -
        // tag only the first row of each profile's chunk so `-o agents`
        // doesn't repeat the identical URL once per item.
        let console_url = crate::execution::console_link_for_profile(targets, &profile, |b| {
            crate::console_url::tco_url(b)
        })
        .await;
        let mut first = true;
        for policy in resp.policies {
            let mut policy_json = policy_to_json(&policy, include_profile, &profile);
            if first {
                if let Some(url) = &console_url {
                    render::tag_console_url(&mut policy_json, url);
                }
                first = false;
            }
            all_json.push(policy_json);
            all_items.push((profile.clone(), policy));
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
                render::print_no_results("No TCO policies found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, policy)| {
                    vec![
                        profile.clone(),
                        policy.id.clone().unwrap_or_default(),
                        policy.display_name().to_string(),
                        policy.display_priority(),
                        policy.display_source_type(),
                        policy.display_severity(),
                        policy.display_archive_retention(),
                    ]
                })
                .collect();
            render::render_table(
                &[
                    "ID",
                    "Name",
                    "Priority",
                    "Source Type",
                    "Severity",
                    "Archive Retention",
                ],
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
    eprintln!("{}", format!("Fetching TCO policy {id}...").dimmed());

    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = TcoPoliciesApi::new(&t.client);
            Ok(api.get(&id).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        if let Some(url) = crate::execution::console_link_for_profile(targets, &profile, |b| {
            crate::console_url::tco_url(b)
        })
        .await
        {
            render::tag_console_url(&mut val, &url);
        }
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
                "TCO policy not found.",
                Some(&|val| {
                    if let Some(policy_val) = val.get("policy") {
                        if let Ok(policy) = serde_json::from_value::<TcoPolicy>(policy_val.clone())
                        {
                            println!("{}:        {}", "Name".bold(), policy.display_name());
                            println!(
                                "{}:          {}",
                                "ID".bold(),
                                policy.id.as_deref().unwrap_or("-")
                            );
                            println!("{}:    {}", "Priority".bold(), policy.display_priority());
                            println!("{}: {}", "Source Type".bold(), policy.display_source_type());
                            println!("{}:    {}", "Severity".bold(), policy.display_severity());
                        }
                    }
                }),
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
    eprintln!("{}", "Creating TCO policy...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = TcoPoliciesApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(policy) = resp.policy {
            let name = policy.display_name().to_string();
            render::print_created(
                "Created",
                "TCO policy",
                Some(&name),
                policy.id.as_deref(),
                &profile,
            );
            let mut policy_json = policy_to_json(&policy, include_profile, &profile);
            if let Some(url) = crate::execution::console_link_for_profile(targets, &profile, |b| {
                crate::console_url::tco_url(b)
            })
            .await
            {
                render::tag_console_url(&mut policy_json, &url);
            }
            all_results.push(policy_json);
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

pub async fn run_update(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Updating TCO policy...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = TcoPoliciesApi::new(&t.client);
            Ok(api.update(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(policy) = resp.policy {
            let name = policy.display_name().to_string();
            render::print_created(
                "Updated",
                "TCO policy",
                Some(&name),
                policy.id.as_deref(),
                &profile,
            );
            let mut policy_json = policy_to_json(&policy, include_profile, &profile);
            if let Some(url) = crate::execution::console_link_for_profile(targets, &profile, |b| {
                crate::console_url::tco_url(b)
            })
            .await
            {
                render::tag_console_url(&mut policy_json, &url);
            }
            all_results.push(policy_json);
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

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    eprintln!("{}", format!("Deleting TCO policy {id}...").dimmed());

    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = TcoPoliciesApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;

    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("TCO policy {id} deleted in profile '{profile}'.").green()
        );
        crate::execution::console_link_for_profile(targets, &profile, |b| {
            crate::console_url::tco_url(b)
        })
        .await;
    }

    Ok(())
}

pub async fn run_reorder(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Reordering TCO policies...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = TcoPoliciesApi::new(&t.client);
            Ok(api.reorder(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Reordered TCO policies in profile '{profile}'.").green()
        );
        if let Some(url) = crate::execution::console_link_for_profile(targets, &profile, |b| {
            crate::console_url::tco_url(b)
        })
        .await
        {
            render::tag_console_url(&mut val, &url);
        }
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

pub async fn run_test(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Testing TCO policy matching...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = TcoPoliciesApi::new(&t.client);
            Ok(api.test_policies(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if let Some(url) = crate::execution::console_link_for_profile(targets, &profile, |b| {
            crate::console_url::tco_url(b)
        })
        .await
        {
            render::tag_console_url(&mut val, &url);
        }
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
            for val in &all_results {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}

pub async fn run_settings(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching TCO settings...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = TcoPoliciesApi::new(&t.client);
        Ok(api.get_settings().await?)
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        if let Some(url) = crate::execution::console_link_for_profile(targets, &profile, |b| {
            crate::console_url::tco_url(b)
        })
        .await
        {
            render::tag_console_url(&mut val, &url);
        }
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
            if all_results.is_empty() {
                render::print_no_results("No TCO settings found.");
                return Ok(());
            }
            for val in &all_results {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}

pub async fn run_settings_update(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Updating TCO settings...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = TcoPoliciesApi::new(&t.client);
            Ok(api.replace_settings(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Updated TCO settings in profile '{profile}'.").green()
        );
        if let Some(url) = crate::execution::console_link_for_profile(targets, &profile, |b| {
            crate::console_url::tco_url(b)
        })
        .await
        {
            render::tag_console_url(&mut val, &url);
        }
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
