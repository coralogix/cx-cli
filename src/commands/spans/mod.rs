use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;

use crate::commands::dataprime::MergedResults;
use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::Tier;

// ── Text renderer ────────────────────────────────────────────────────────────

/// Render merged span rows as human-readable text.
///
/// Each row is printed as: `<traceID> <spanID> <service> <operation> <duration>`
/// with colour coding and optional profile prefix.
pub fn render_span_text(merged: &MergedResults) -> Result<()> {
    if merged.rows.is_empty() {
        println!("{}", "No spans found.".yellow());
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

        let user_data = row.get("userData").unwrap_or(&Value::Null);

        let trace_id = user_data
            .get("traceID")
            .or_else(|| user_data.get("trace_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");

        let span_id = user_data
            .get("spanID")
            .or_else(|| user_data.get("span_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");

        let operation = row
            .pointer("/labels/operationName")
            .or_else(|| user_data.get("operationName"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");

        let duration = row
            .pointer("/metadata/duration")
            .or_else(|| user_data.get("duration"))
            .and_then(|v| v.as_u64())
            .map(|d| format!("{}ms", d / 1000))
            .unwrap_or_else(|| "-".to_string());

        let service = row
            .pointer("/labels/serviceName")
            .or_else(|| row.pointer("/labels/processServiceName"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");

        println!(
            "{}{} {} {} {} {}",
            profile.dimmed(),
            trace_id.cyan(),
            span_id.dimmed(),
            service.green(),
            operation,
            duration.yellow()
        );
    }

    Ok(())
}

// ── Top-level orchestrator ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run(
    targets: &[Arc<ExecutionTarget>],
    query: &str,
    start: &str,
    end: &str,
    limit: u32,
    tier: Tier,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
) -> Result<()> {
    super::dataprime::run_query(
        targets,
        query,
        "spans",
        start,
        end,
        limit,
        tier,
        output,
        max_direct,
        temp_dir,
        Some(render_span_text),
    )
    .await
}
