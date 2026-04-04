use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use toon_format::encode_default as toon_encode;

use crate::api::{dataprime::DataprimeApi, dataprime::QuerySpansResponse};
use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::spill::{maybe_spill, transform_for_agents, SpillOutcome};
use crate::time::parse_timestamp;
use crate::Tier;

// ── Query Normalization ──────────────────────────────────────────────────────

/// Normalize query to ensure it starts with 'source spans'.
/// If the query already starts with 'source', it's returned as-is.
/// Otherwise, 'source spans |' is prepended.
pub fn normalize_query(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.to_lowercase().starts_with("source ") {
        trimmed.to_string()
    } else {
        format!("source spans | {trimmed}")
    }
}

// ── Execute ───────────────────────────────────────────────────────────────────

async fn execute(
    target: Arc<ExecutionTarget>,
    query: &str,
    start: &str,
    end: &str,
    limit: u32,
    tier: Tier,
) -> Result<QuerySpansResponse> {
    let api = DataprimeApi::new(&target.client);
    let start_ts = parse_timestamp(start)?;
    let end_ts = parse_timestamp(end)?;
    Ok(api
        .query_spans(query, &start_ts, &end_ts, limit, tier)
        .await?)
}

// ── Merge ─────────────────────────────────────────────────────────────────────

struct MergedSpans {
    rows: Vec<Value>,
    warnings: Vec<String>,
    is_aggregate: bool,
    include_profile: bool,
}

fn merge(
    per_profile: Vec<(String, Result<QuerySpansResponse>)>,
    include_profile: bool,
) -> MergedSpans {
    let mut rows: Vec<Value> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut is_aggregate: Option<bool> = None;

    for (profile, result) in per_profile {
        match result {
            Ok(resp) => {
                for w in resp.warnings {
                    warnings.push(format!("[{profile}] {w}"));
                }
                if is_aggregate.is_none() {
                    is_aggregate = Some(resp.is_aggregate);
                }
                if include_profile {
                    rows.extend(resp.raw_results.into_iter().map(|mut row| {
                        if let Value::Object(ref mut m) = row {
                            m.insert("profile".to_string(), Value::String(profile.clone()));
                        }
                        row
                    }));
                } else {
                    rows.extend(resp.raw_results);
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    MergedSpans {
        rows,
        warnings,
        is_aggregate: is_aggregate.unwrap_or(false),
        include_profile,
    }
}

// ── Render ────────────────────────────────────────────────────────────────────

fn render(
    merged: &MergedSpans,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
) -> Result<()> {
    for w in &merged.warnings {
        eprintln!("{}", w.yellow());
    }

    match output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&merged.rows)?);
        }
        OutputFormat::Agents => {
            if merged.is_aggregate {
                let toon = toon_encode(&merged.rows)
                    .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
                println!("{toon}");
            } else {
                let agent_rows: Vec<_> = merged.rows.iter().map(transform_for_agents).collect();
                match maybe_spill(&agent_rows, max_direct, temp_dir)? {
                    SpillOutcome::Direct(json) => println!("{json}"),
                    SpillOutcome::Spilled { path, count } => {
                        println!(
                            "{count} spans retrieved. Results written to: {}",
                            path.display()
                        );
                    }
                }
            }
        }
        OutputFormat::Text => {
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
        }
    }

    Ok(())
}

// ── Top-level orchestrator ────────────────────────────────────────────────────

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
    eprintln!("{}", "Querying spans...".dimmed());

    let include_profile = targets.len() > 1;
    let normalized_query = normalize_query(query);
    let start = start.to_string();
    let end = end.to_string();
    let per_profile = fan_out(targets, |t| {
        let q = normalized_query.clone();
        let s = start.clone();
        let e = end.clone();
        async move { execute(t, &q, &s, &e, limit, tier).await }
    })
    .await;

    let merged = merge(per_profile, include_profile);
    render(&merged, output, max_direct, temp_dir)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_query_prepends_source_spans() {
        assert_eq!(
            normalize_query("filter $d.traceID == \"abc\""),
            "source spans | filter $d.traceID == \"abc\""
        );
    }

    #[test]
    fn normalize_query_preserves_existing_source() {
        assert_eq!(
            normalize_query("source spans | filter $d.traceID == \"abc\""),
            "source spans | filter $d.traceID == \"abc\""
        );
    }

    #[test]
    fn normalize_query_handles_different_source() {
        assert_eq!(
            normalize_query("source logs | filter $d.msg ~ 'error'"),
            "source logs | filter $d.msg ~ 'error'"
        );
    }

    #[test]
    fn normalize_query_case_insensitive() {
        assert_eq!(
            normalize_query("SOURCE spans | filter $d.error == true"),
            "SOURCE spans | filter $d.error == true"
        );
    }

    #[test]
    fn normalize_query_trims_whitespace() {
        assert_eq!(
            normalize_query("  filter $d.traceID == \"abc\"  "),
            "source spans | filter $d.traceID == \"abc\""
        );
    }
}
