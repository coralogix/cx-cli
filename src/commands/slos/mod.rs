pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{Slo, SlosApi};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn slo_to_json(slo: &Slo, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": slo.id,
        "name": slo.display_name(),
        "description": slo.description,
        "creator": slo.creator,
        "target": slo.target_threshold_percentage,
        "type": slo.display_type(),
        "time_frame": slo.display_time_frame(),
        "product_type": slo.display_product_type(),
        "create_time": slo.create_time,
        "update_time": slo.update_time,
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
        eprintln!("{}", "Reading SLO definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading SLO definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching SLOs...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = SlosApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, Slo)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        // Print the SLOs list page link to stderr once per profile. Skip
        // when there are no SLOs, since there's nothing to view.
        if !resp.slos.is_empty() {
            crate::execution::console_link_for_profile(targets, &profile, |b| {
                crate::console_url::slos_url(b)
            })
            .await;
        }
        for slo in resp.slos {
            all_json.push(slo_to_json(&slo, include_profile, &profile));
            all_items.push((profile.clone(), slo));
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
                render::print_no_results("No SLOs found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, slo)| {
                    vec![
                        profile.clone(),
                        slo.id.clone().unwrap_or_default(),
                        slo.display_name().to_string(),
                        slo.display_target(),
                        slo.display_type(),
                        slo.display_time_frame(),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Target", "Type", "Period"],
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
    eprintln!("{}", format!("Fetching SLO {id}...").dimmed());

    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = SlosApi::new(&t.client);
            Ok(api.get(&id).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
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
                "SLO not found.",
                Some(&|val| {
                    if let Some(slo_val) = val.get("slo") {
                        if let Ok(slo) = serde_json::from_value::<Slo>(slo_val.clone()) {
                            println!("{}:   {}", "Name".bold(), slo.display_name());
                            println!("{}:     {}", "ID".bold(), slo.id.as_deref().unwrap_or("-"));
                            println!("{}: {}", "Target".bold(), slo.display_target());
                            println!("{}:   {}", "Type".bold(), slo.display_type());
                            println!("{}: {}", "Period".bold(), slo.display_time_frame());
                            println!("{}:", "Creator".bold(),);
                            println!("        {}", slo.creator.as_deref().unwrap_or("-"));
                            if let Some(ref desc) = slo.description {
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

pub async fn run_create(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;

    eprintln!("{}", "Creating SLO...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = SlosApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(slo) = resp.slo {
            let name = slo.display_name().to_string();
            render::print_created("Created", "SLO", Some(&name), slo.id.as_deref(), &profile);
            if let Some(id) = slo.id.as_deref() {
                crate::execution::console_link_for_profile(targets, &profile, |b| {
                    crate::console_url::slo_url(b, id)
                })
                .await;
            }
            let slo_json = slo_to_json(&slo, include_profile, &profile);
            all_results.push(slo_json);
        } else {
            eprintln!(
                "{}",
                format!("SLO created in profile '{profile}' but response was empty.").yellow()
            );
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
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;

    eprintln!("{}", "Updating SLO...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = SlosApi::new(&t.client);
            Ok(api.replace(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(slo) = resp.slo {
            let name = slo.display_name().to_string();
            render::print_created("Updated", "SLO", Some(&name), slo.id.as_deref(), &profile);
            if let Some(id) = slo.id.as_deref() {
                crate::execution::console_link_for_profile(targets, &profile, |b| {
                    crate::console_url::slo_url(b, id)
                })
                .await;
            }
            let slo_json = slo_to_json(&slo, include_profile, &profile);
            all_results.push(slo_json);
        } else {
            eprintln!(
                "{}",
                format!("SLO updated in profile '{profile}' but response was empty.").yellow()
            );
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

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    eprintln!("{}", format!("Deleting SLO {id}...").dimmed());

    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = SlosApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;

    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("SLO {id} deleted in profile '{profile}'.").green()
        );
    }

    Ok(())
}
