pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::render;
use api::EnrichmentsApi;

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading enrichment definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading enrichment definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

fn render_results(
    all_results: &[Value],
    output: OutputFormat,
    include_profile: bool,
) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json_auto(all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_results.is_empty() {
                render::print_no_results("No data found.");
                return Ok(());
            }
            if include_profile {
                for val in all_results {
                    println!("{}", serde_json::to_string_pretty(val)?);
                }
            } else if let Some(val) = all_results.first() {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }
    Ok(())
}

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching enrichments...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = EnrichmentsApi::new(&t.client);
        Ok(api.list().await?)
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
    render_results(&all_results, output, include_profile)
}

pub async fn run_add(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Adding enrichment rules...".dimmed());
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = EnrichmentsApi::new(&t.client);
            Ok(api.add(&body).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(val) => {
                eprintln!(
                    "{}",
                    format!("Added enrichments in profile '{profile}'.").green()
                );
                all_results.push(val);
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }
    render_results(&all_results, output, targets.len() > 1)
}

pub async fn run_remove(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Removing enrichment rules...".dimmed());
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = EnrichmentsApi::new(&t.client);
            Ok(api.remove(&body).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(val) => {
                eprintln!(
                    "{}",
                    format!("Removed enrichments in profile '{profile}'.").green()
                );
                all_results.push(val);
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }
    render_results(&all_results, output, targets.len() > 1)
}

pub async fn run_overwrite(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Overwriting enrichment rules...".dimmed());
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = EnrichmentsApi::new(&t.client);
            Ok(api.overwrite(&body).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(val) => {
                eprintln!(
                    "{}",
                    format!("Overwrote enrichments in profile '{profile}'.").green()
                );
                all_results.push(val);
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }
    render_results(&all_results, output, targets.len() > 1)
}

pub async fn run_limit(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching enrichment limits...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = EnrichmentsApi::new(&t.client);
        Ok(api.limit().await?)
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
    render_results(&all_results, output, include_profile)
}

pub async fn run_settings(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching enrichment settings...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = EnrichmentsApi::new(&t.client);
        Ok(api.settings().await?)
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
    render_results(&all_results, output, include_profile)
}
