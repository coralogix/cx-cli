use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;

use crate::commands::dataprime::MergedResults;
use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::Tier;

// ── Text renderer ────────────────────────────────────────────────────────────

/// Render merged log rows as human-readable text.
///
/// Each row is printed as: `<timestamp> [<severity>] <message>`
/// with colour-coded severity and optional profile prefix.
pub fn render_log_text(merged: &MergedResults) -> Result<()> {
    if merged.rows.is_empty() {
        println!("{}", "No results found.".yellow());
        return Ok(());
    }

    if merged.is_aggregate {
        for row in &merged.rows {
            println!("{}", serde_json::to_string_pretty(row)?);
        }
        return Ok(());
    }

    for row in &merged.rows {
        let profile = if merged.include_profile {
            row.get("profile")
                .and_then(|v| v.as_str())
                .map(|s| format!("[{s}] "))
                .unwrap_or_default()
        } else {
            String::new()
        };

        let ts = row
            .pointer("/metadata/timestamp")
            .or_else(|| row.pointer("/userData/coralogix/timestamp"))
            .or_else(|| row.pointer("/userData/timestamp"))
            .or_else(|| row.pointer("/userData/@timestamp"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");

        let sev = row
            .pointer("/metadata/severity")
            .and_then(|v| v.as_str())
            .map(map_severity)
            .unwrap_or_else(|| "INFO".to_string());

        let text = row
            .pointer("/userData/message")
            .or_else(|| row.pointer("/userData/msg"))
            .or_else(|| row.get("userData"))
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();

        let sev_colored = match sev.to_uppercase().as_str() {
            "ERROR" | "CRITICAL" => sev.red().bold().to_string(),
            "WARNING" | "WARN" => sev.yellow().to_string(),
            "DEBUG" => sev.dimmed().to_string(),
            _ => sev.cyan().to_string(),
        };

        if profile.is_empty() {
            println!("{} [{}] {}", ts.dimmed(), sev_colored, text);
        } else {
            println!(
                "{}{} [{}] {}",
                profile.dimmed(),
                ts.dimmed(),
                sev_colored,
                text
            );
        }
    }

    Ok(())
}

fn map_severity(raw: &str) -> String {
    match raw {
        "1" => "DEBUG".to_string(),
        "2" => "VERBOSE".to_string(),
        "3" => "INFO".to_string(),
        "4" => "WARNING".to_string(),
        "5" => "ERROR".to_string(),
        "6" => "CRITICAL".to_string(),
        other => other.to_uppercase(),
    }
}

// ── Top-level orchestrator ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run(
    targets: &[Arc<ExecutionTarget>],
    query: &str,
    start: &str,
    end: &str,
    limit: u32,
    tier: Option<Tier>,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
) -> Result<()> {
    super::dataprime::run_query(
        targets,
        query,
        "logs",
        start,
        end,
        limit,
        tier,
        output,
        max_direct,
        temp_dir,
        Some(render_log_text),
    )
    .await
}
