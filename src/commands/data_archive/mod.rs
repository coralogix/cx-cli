pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::DataArchiveApi;

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!("{}", format!("Reading definition from {path}...").dimmed());
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

/// Print the "View in Coralogix" link for the Archive settings page, if a
/// console base URL can be resolved for `profile`. Returns the URL so
/// callers can also embed it as a `consoleUrl` field in `-o json` /
/// `-o agents` output via [`render::tag_console_url`].
///
/// Metrics- and logs-archive configuration both live on a single static
/// page (`#/physical-locations`) - the editor is an in-page dialog, not a
/// per-entity route - so every write here links to that same page.
async fn print_archive_console_link(
    targets: &[Arc<ExecutionTarget>],
    profile: &str,
) -> Option<String> {
    if let Some(target) = crate::execution::find_target(targets, profile) {
        if let Some(base) = target.console_base().await {
            let url = crate::console_url::archive_url(&base);
            render::print_console_link(&url);
            return Some(url);
        }
    }
    None
}

// --- Metrics ---

pub async fn run_metrics_get(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching metrics archive config...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = DataArchiveApi::new(&t.client);
        Ok(api.get_config().await?)
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        if let Some(url) = print_archive_console_link(targets, &profile).await {
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
                "No metrics archive config found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}

pub async fn run_metrics_create(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Creating metrics archive config...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = DataArchiveApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Created metrics archive config in profile '{profile}'.").green()
        );
        if let Some(url) = print_archive_console_link(targets, &profile).await {
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

pub async fn run_metrics_update(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Updating metrics archive config...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = DataArchiveApi::new(&t.client);
            Ok(api.update(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Updated metrics archive config in profile '{profile}'.").green()
        );
        if let Some(url) = print_archive_console_link(targets, &profile).await {
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

pub async fn run_metrics_enable(targets: &[Arc<ExecutionTarget>]) -> Result<()> {
    eprintln!("{}", "Enabling metrics archive...".dimmed());

    let per_profile = fan_out(targets, |t| async move {
        let api = DataArchiveApi::new(&t.client);
        api.enable().await?;
        Ok(())
    })
    .await;

    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Metrics archive enabled in profile '{profile}'.").green()
        );
        print_archive_console_link(targets, &profile).await;
    }
    Ok(())
}

pub async fn run_metrics_disable(targets: &[Arc<ExecutionTarget>]) -> Result<()> {
    eprintln!("{}", "Disabling metrics archive...".dimmed());

    let per_profile = fan_out(targets, |t| async move {
        let api = DataArchiveApi::new(&t.client);
        api.disable().await?;
        Ok(())
    })
    .await;

    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Metrics archive disabled in profile '{profile}'.").green()
        );
        print_archive_console_link(targets, &profile).await;
    }
    Ok(())
}

pub async fn run_metrics_validate(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Validating metrics archive config...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = DataArchiveApi::new(&t.client);
            Ok(api.validate(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        eprintln!(
            "{}",
            format!("Validation complete in profile '{profile}'.").green()
        );
        if let Some(url) = print_archive_console_link(targets, &profile).await {
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

// --- Logs ---

pub async fn run_logs_get(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching logs archive target...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = DataArchiveApi::new(&t.client);
        Ok(api.get_target().await?)
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        if let Some(url) = print_archive_console_link(targets, &profile).await {
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
                "No logs archive target found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}

pub async fn run_logs_set(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Setting logs archive target...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = DataArchiveApi::new(&t.client);
            Ok(api.set_target(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Logs archive target set in profile '{profile}'.").green()
        );
        if let Some(url) = print_archive_console_link(targets, &profile).await {
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
