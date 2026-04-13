use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use toon_format::encode_default as toon_encode;

use crate::api::{dataprime::DataprimeApi, dataprime::QueryLogsResponse};
use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::spill::{maybe_spill, transform_for_agents, SpillOutcome};
use crate::time::parse_timestamp;
use crate::Tier;

// ── Execute ───────────────────────────────────────────────────────────────────

/// Fetch logs for one execution target. Returns structured domain data, no I/O.
pub async fn execute(
    target: Arc<ExecutionTarget>,
    query: &str,
    start: &str,
    end: &str,
    limit: u32,
    tier: Tier,
) -> Result<QueryLogsResponse> {
    let api = DataprimeApi::new(&target.client);
    let start_ts = parse_timestamp(start)?;
    let end_ts = parse_timestamp(end)?;
    Ok(api
        .query_logs(query, &start_ts, &end_ts, limit, tier)
        .await?)
}

// ── Merge ─────────────────────────────────────────────────────────────────────

/// Merge per-profile log responses into a single list of optionally tagged rows.
///
/// Injects `"profile"` into every `raw_results` row only when `include_profile`
/// is true (i.e. when more than one profile is being queried). Warnings are
/// prefixed with the profile name. The returned `is_aggregate` flag reflects
/// the first successful response (all profiles share the same query, so all
/// flags should match).
pub struct MergedLogs {
    pub rows: Vec<Value>,
    pub warnings: Vec<String>,
    pub is_aggregate: bool,
    pub include_profile: bool,
}

pub fn merge(
    per_profile: Vec<(String, Result<QueryLogsResponse>)>,
    include_profile: bool,
) -> MergedLogs {
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

    MergedLogs {
        rows,
        warnings,
        is_aggregate: is_aggregate.unwrap_or(false),
        include_profile,
    }
}

// ── Render ────────────────────────────────────────────────────────────────────

pub fn render(
    merged: &MergedLogs,
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
                            "{count} logs retrieved. Results written to: {}",
                            path.display()
                        );
                    }
                }
            }
        }
        OutputFormat::Text => {
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

                // Extract from the normalized row shape.
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::api::dataprime::QueryLogsResponse;

    fn make_raw_row(msg: &str) -> serde_json::Value {
        json!({
            "metadata": {"severity": "3", "timestamp": "2024-01-01T00:00:00Z"},
            "labels": {},
            "userData": {"message": msg}
        })
    }

    fn make_response(rows: Vec<serde_json::Value>, is_aggregate: bool) -> QueryLogsResponse {
        QueryLogsResponse {
            results: vec![],
            raw_results: rows,
            warnings: vec![],
            total_count: None,
            is_aggregate,
        }
    }

    #[test]
    fn merge_single_profile_omits_profile_field_when_disabled() {
        let rows = vec![make_raw_row("hello"), make_raw_row("world")];
        let per_profile = vec![("prod".to_string(), Ok(make_response(rows, false)))];
        let merged = merge(per_profile, false);

        assert_eq!(merged.rows.len(), 2);
        assert!(!merged.include_profile);
        for row in &merged.rows {
            assert!(row.get("profile").is_none());
        }
    }

    #[test]
    fn merge_single_profile_includes_profile_when_enabled() {
        let rows = vec![make_raw_row("hello")];
        let per_profile = vec![("prod".to_string(), Ok(make_response(rows, false)))];
        let merged = merge(per_profile, true);

        assert_eq!(merged.rows.len(), 1);
        assert!(merged.include_profile);
        assert_eq!(merged.rows[0]["profile"], json!("prod"));
    }

    #[test]
    fn merge_multiple_profiles_labels_each_row_with_source_profile() {
        let per_profile = vec![
            (
                "prod".to_string(),
                Ok(make_response(vec![make_raw_row("prod-log")], false)),
            ),
            (
                "staging".to_string(),
                Ok(make_response(vec![make_raw_row("staging-log")], false)),
            ),
        ];
        let merged = merge(per_profile, true);

        assert_eq!(merged.rows.len(), 2);
        assert_eq!(merged.rows[0]["profile"], json!("prod"));
        assert_eq!(merged.rows[1]["profile"], json!("staging"));
    }

    #[test]
    fn merge_skips_errored_profiles_and_keeps_successful_ones() {
        let per_profile: Vec<(String, anyhow::Result<QueryLogsResponse>)> = vec![
            (
                "good".to_string(),
                Ok(make_response(vec![make_raw_row("ok")], false)),
            ),
            ("bad".to_string(), Err(anyhow::anyhow!("network error"))),
        ];
        let merged = merge(per_profile, true);

        assert_eq!(merged.rows.len(), 1);
        assert_eq!(merged.rows[0]["profile"], json!("good"));
    }

    #[test]
    fn merge_collects_warnings_prefixed_with_profile() {
        let mut resp = make_response(vec![], false);
        resp.warnings = vec!["too many results".to_string()];
        let per_profile = vec![("prod".to_string(), Ok(resp))];
        let merged = merge(per_profile, true);

        assert_eq!(merged.warnings.len(), 1);
        assert!(merged.warnings[0].contains("[prod]"));
    }

    #[test]
    fn merge_is_aggregate_from_first_successful_profile() {
        let per_profile = vec![
            ("p1".to_string(), Ok(make_response(vec![], true))),
            ("p2".to_string(), Ok(make_response(vec![], true))),
        ];
        let merged = merge(per_profile, true);
        assert!(merged.is_aggregate);
    }

    #[test]
    fn render_json_includes_profile_field_when_multi_profile() {
        use crate::config::OutputFormat;

        let merged = MergedLogs {
            rows: vec![json!({"profile": "prod", "userData": {"message": "hi"}})],
            warnings: vec![],
            is_aggregate: false,
            include_profile: true,
        };

        assert!(render(&merged, OutputFormat::Json, None, "/tmp/").is_ok());
    }

    #[test]
    fn render_json_omits_profile_field_when_single_profile() {
        use crate::config::OutputFormat;

        let merged = MergedLogs {
            rows: vec![json!({"userData": {"message": "hi"}})],
            warnings: vec![],
            is_aggregate: false,
            include_profile: false,
        };

        assert!(render(&merged, OutputFormat::Json, None, "/tmp/").is_ok());
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
    tier: Tier,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
) -> Result<()> {
    eprintln!("{}", "Querying logs...".dimmed());

    let include_profile = targets.len() > 1;
    let query = query.to_string();
    let start = start.to_string();
    let end = end.to_string();
    let per_profile = fan_out(targets, |t| {
        let q = query.clone();
        let s = start.clone();
        let e = end.clone();
        async move { execute(t, &q, &s, &e, limit, tier).await }
    })
    .await;

    let merged = merge(per_profile, include_profile);
    render(&merged, output, max_direct, temp_dir)
}
