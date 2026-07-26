use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tabled::{Table, Tabled};
use toon_format::encode_default as toon_encode;

pub mod api;
pub mod semantic_search;

use api::{DataprimeApi, QueryGenericResponse};

use crate::cases_query_rules::check_cases_query_rules;
use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::spill::{maybe_spill, transform_for_agents, SpillOutcome};
use crate::time::parse_timestamp;
use crate::Tier;

/// YAML bundle shipped in the binary (`assets/dataprime_docs.yaml`).
const EMBEDDED_DATAPRIME_DOCS_YAML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/dataprime_docs.yaml"
));

/// A single documentation entry for a command or function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocEntry {
    pub description: String,
    pub syntax: String,
    pub category: Vec<String>,
}

/// The complete DataPrime documentation loaded from YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataprimeDocs {
    pub commands: HashMap<String, DocEntry>,
    pub functions: HashMap<String, DocEntry>,
}

/// Filter type for listing commands/functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum DataprimeFilter {
    #[default]
    All,
    Commands,
    Functions,
}

/// Row structure for the list table output.
#[derive(Tabled)]
struct ListRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    kind: String,
    #[tabled(rename = "Description")]
    description: String,
}

/// Load the DataPrime documentation from the bundle embedded at build time (`assets/dataprime_docs.yaml`).
pub fn load_docs() -> Result<DataprimeDocs> {
    serde_yaml::from_str(EMBEDDED_DATAPRIME_DOCS_YAML)
        .context("Failed to parse embedded dataprime documentation")
}

/// Truncate a string to a maximum length, adding ellipsis if needed.
fn truncate(s: &str, max_len: usize) -> String {
    let s = s.lines().next().unwrap_or(s);
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Extract the first sentence from a description.
fn first_sentence(s: &str) -> String {
    let s = s.trim();
    if let Some(pos) = s.find(". ") {
        s[..=pos].to_string()
    } else if let Some(pos) = s.find(".\n") {
        s[..=pos].to_string()
    } else {
        s.lines().next().unwrap_or(s).to_string()
    }
}

/// List available DataPrime commands and/or functions.
pub fn run_list(
    filter: DataprimeFilter,
    name_pattern: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let docs = load_docs()?;

    let mut items: Vec<(String, String, &DocEntry)> = Vec::new();

    if filter == DataprimeFilter::All || filter == DataprimeFilter::Commands {
        for (name, entry) in &docs.commands {
            if let Some(pattern) = name_pattern {
                if !name.to_lowercase().contains(&pattern.to_lowercase()) {
                    continue;
                }
            }
            items.push((name.clone(), "command".to_string(), entry));
        }
    }

    if filter == DataprimeFilter::All || filter == DataprimeFilter::Functions {
        for (name, entry) in &docs.functions {
            if let Some(pattern) = name_pattern {
                if !name.to_lowercase().contains(&pattern.to_lowercase()) {
                    continue;
                }
            }
            items.push((name.clone(), "function".to_string(), entry));
        }
    }

    items.sort_by(|a, b| a.0.cmp(&b.0));

    match output {
        OutputFormat::Json => {
            let json_items: Vec<_> = items
                .iter()
                .map(|(name, kind, entry)| {
                    serde_json::json!({
                        "name": name,
                        "type": kind,
                        "syntax": entry.syntax,
                        "description": first_sentence(&entry.description),
                        "category": entry.category,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_items)?);
        }
        OutputFormat::Agents => {
            let agent_items: Vec<_> = items
                .iter()
                .map(|(name, kind, entry)| {
                    serde_json::json!({
                        "name": name,
                        "type": kind,
                        "syntax": entry.syntax,
                        "description": first_sentence(&entry.description),
                    })
                })
                .collect();
            let toon = toon_encode(&agent_items)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if items.is_empty() {
                println!("{}", "No matching items found.".yellow());
                return Ok(());
            }

            let rows: Vec<ListRow> = items
                .iter()
                .map(|(name, kind, entry)| ListRow {
                    name: name.clone(),
                    kind: kind.clone(),
                    description: truncate(&first_sentence(&entry.description), 60),
                })
                .collect();

            let table = Table::new(rows).to_string();
            println!("{table}");
            println!(
                "\n{} items ({} commands, {} functions)",
                items.len(),
                items.iter().filter(|(_, k, _)| k == "command").count(),
                items.iter().filter(|(_, k, _)| k == "function").count()
            );
        }
    }

    Ok(())
}

/// Show detailed documentation for a specific command or function.
pub fn run_help(name: &str, output: OutputFormat) -> Result<()> {
    let docs = load_docs()?;

    let (kind, entry) = if let Some(entry) = docs.commands.get(name) {
        ("command", entry)
    } else if let Some(entry) = docs.functions.get(name) {
        ("function", entry)
    } else {
        let all_names: Vec<_> = docs.commands.keys().chain(docs.functions.keys()).collect();

        let suggestions: Vec<_> = all_names
            .iter()
            .filter(|n| n.to_lowercase().contains(&name.to_lowercase()))
            .take(5)
            .collect();

        let mut msg = format!("No command or function named '{}' found.", name);
        if !suggestions.is_empty() {
            msg.push_str("\n\nDid you mean one of these?\n");
            for s in suggestions {
                msg.push_str(&format!("  - {}\n", s));
            }
        }
        anyhow::bail!(msg);
    };

    match output {
        OutputFormat::Json => {
            let json = serde_json::json!({
                "name": name,
                "type": kind,
                "syntax": entry.syntax,
                "description": entry.description,
                "category": entry.category,
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        OutputFormat::Agents => {
            let json = serde_json::json!({
                "name": name,
                "type": kind,
                "syntax": entry.syntax,
                "description": entry.description,
            });
            let toon =
                toon_encode(&json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            println!("{}", format!("{} ({})", name, kind).bold());
            println!();
            println!("{}", "Syntax:".cyan());
            println!("  {}", entry.syntax);
            println!();
            println!("{}", "Category:".cyan());
            println!("  {}", entry.category.join(" > "));
            println!();
            println!("{}", "Description:".cyan());
            for line in entry.description.lines() {
                println!("  {}", line);
            }
        }
    }

    Ok(())
}

// ── DataPrime query execution ────────────────────────────────────────────────

/// Execute a generic DataPrime query against one target.
async fn execute_query(
    target: Arc<ExecutionTarget>,
    query: &str,
    start_ts: &str,
    end_ts: &str,
    limit: u32,
    tier: Tier,
    source: &str,
) -> Result<QueryGenericResponse> {
    let api = DataprimeApi::new(&target.client);
    Ok(api
        .query_generic(query, start_ts, end_ts, limit, tier, source)
        .await?)
}

/// Merged results from one or more profiles.
pub struct MergedResults {
    pub rows: Vec<Value>,
    pub warnings: Vec<String>,
    pub is_aggregate: bool,
    pub include_profile: bool,
}

/// Merge per-profile generic responses into a single result set.
pub fn merge_results(
    per_profile: Vec<(String, Result<QueryGenericResponse>)>,
    include_profile: bool,
    cases_warning: Option<&str>,
) -> Result<MergedResults> {
    let mut rows: Vec<Value> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    if let Some(w) = cases_warning {
        warnings.push(w.to_string());
    }
    let mut is_aggregate: Option<bool> = None;

    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
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

    Ok(MergedResults {
        rows,
        warnings,
        is_aggregate: is_aggregate.unwrap_or(false),
        include_profile,
    })
}

/// Render merged results to stdout.
///
/// JSON and Agents modes are handled generically. For Text mode, if a
/// `text_renderer` is provided it is used for source-specific formatting
/// (e.g. logs show timestamp/severity, spans show traceID/duration).
/// Otherwise rows are printed as pretty-printed JSON.
pub fn render_results(
    merged: &MergedResults,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
    text_renderer: Option<fn(&MergedResults) -> Result<()>>,
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
                            "{count} results retrieved. Results written to: {}",
                            path.display()
                        );
                    }
                }
            }
        }
        OutputFormat::Text => {
            if let Some(renderer) = text_renderer {
                return renderer(merged);
            }
            // Generic text rendering - pretty-print each row.
            if merged.rows.is_empty() {
                println!("{}", "No results found.".yellow());
                return Ok(());
            }
            for row in &merged.rows {
                println!("{}", serde_json::to_string_pretty(row)?);
            }
        }
    }

    Ok(())
}

/// Run a generic DataPrime query across all targets with fan-out, merge, and render.
#[allow(clippy::too_many_arguments)]
pub async fn run_query(
    targets: &[Arc<ExecutionTarget>],
    query: &str,
    source: &str,
    start: &str,
    end: &str,
    limit: u32,
    tier: Option<Tier>,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
    text_renderer: Option<fn(&MergedResults) -> Result<()>>,
) -> Result<()> {
    eprintln!("{}", "Querying...".dimmed());

    let start_fmt = parse_timestamp(start)?;
    let end_fmt = parse_timestamp(end)?;
    let cases_warning = check_cases_query_rules(query, &start_fmt, &end_fmt);

    let include_profile = targets.len() > 1;
    let query = query.to_string();
    let source = source.to_string();
    let per_profile = fan_out(targets, |t| {
        let q = query.clone();
        let src = source.clone();
        let s = start_fmt.clone();
        let e = end_fmt.clone();
        async move {
            let effective_tier = tier.unwrap_or(t.cfg.default_tier);
            execute_query(t, &q, &s, &e, limit, effective_tier, &src).await
        }
    })
    .await;

    let merged = merge_results(per_profile, include_profile, cases_warning.as_deref())?;
    render_results(&merged, output, max_direct, temp_dir, text_renderer)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string() {
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn truncate_multiline() {
        assert_eq!(truncate("line1\nline2\nline3", 100), "line1");
    }

    #[test]
    fn doc_entry_deserialize() {
        let yaml = r#"
description: "Test description"
syntax: "test <arg>"
category: ["Commands reference", "test"]
"#;
        let entry: DocEntry = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(entry.description, "Test description");
        assert_eq!(entry.syntax, "test <arg>");
        assert_eq!(entry.category, vec!["Commands reference", "test"]);
    }

    #[test]
    fn first_sentence_with_period_space() {
        assert_eq!(
            first_sentence("First sentence. Second sentence."),
            "First sentence."
        );
    }

    #[test]
    fn first_sentence_with_period_newline() {
        assert_eq!(
            first_sentence("First sentence.\nSecond sentence."),
            "First sentence."
        );
    }

    #[test]
    fn first_sentence_single_line() {
        assert_eq!(first_sentence("Only one sentence."), "Only one sentence.");
    }

    #[test]
    fn first_sentence_no_period() {
        assert_eq!(first_sentence("No period here"), "No period here");
    }

    #[test]
    fn first_sentence_multiline_no_period() {
        assert_eq!(first_sentence("First line\nSecond line"), "First line");
    }

    // ── merge_results tests ──────────────────────────────────────────────────

    use serde_json::json;

    use super::api::QueryGenericResponse;

    fn make_generic_response(
        rows: Vec<serde_json::Value>,
        is_aggregate: bool,
    ) -> QueryGenericResponse {
        QueryGenericResponse {
            raw_results: rows,
            warnings: vec![],
            is_aggregate,
        }
    }

    #[test]
    fn merge_single_profile_omits_profile_field() {
        let rows = vec![json!({"userData": {"message": "hello"}})];
        let per_profile = vec![("prod".to_string(), Ok(make_generic_response(rows, false)))];
        let merged = merge_results(per_profile, false, None).unwrap();

        assert_eq!(merged.rows.len(), 1);
        assert!(!merged.include_profile);
        assert!(merged.rows[0].get("profile").is_none());
    }

    #[test]
    fn merge_multiple_profiles_tags_rows() {
        let per_profile = vec![
            (
                "prod".to_string(),
                Ok(make_generic_response(
                    vec![json!({"userData": {"msg": "a"}})],
                    false,
                )),
            ),
            (
                "staging".to_string(),
                Ok(make_generic_response(
                    vec![json!({"userData": {"msg": "b"}})],
                    false,
                )),
            ),
        ];
        let merged = merge_results(per_profile, true, None).unwrap();

        assert_eq!(merged.rows.len(), 2);
        assert_eq!(merged.rows[0]["profile"], json!("prod"));
        assert_eq!(merged.rows[1]["profile"], json!("staging"));
    }

    #[test]
    fn merge_skips_errored_profiles() {
        let per_profile: Vec<(String, anyhow::Result<QueryGenericResponse>)> = vec![
            (
                "good".to_string(),
                Ok(make_generic_response(vec![json!({"data": 1})], false)),
            ),
            ("bad".to_string(), Err(anyhow::anyhow!("network error"))),
        ];
        let merged = merge_results(per_profile, true, None).unwrap();

        assert_eq!(merged.rows.len(), 1);
        assert_eq!(merged.rows[0]["profile"], json!("good"));
    }

    #[test]
    fn merge_collects_warnings_with_profile_prefix() {
        let mut resp = make_generic_response(vec![], false);
        resp.warnings = vec!["too many results".to_string()];
        let per_profile = vec![("prod".to_string(), Ok(resp))];
        let merged = merge_results(per_profile, true, None).unwrap();

        assert_eq!(merged.warnings.len(), 1);
        assert!(merged.warnings[0].contains("[prod]"));
    }

    #[test]
    fn merge_is_aggregate_from_first_successful() {
        let per_profile = vec![
            ("p1".to_string(), Ok(make_generic_response(vec![], true))),
            ("p2".to_string(), Ok(make_generic_response(vec![], true))),
        ];
        let merged = merge_results(per_profile, true, None).unwrap();
        assert!(merged.is_aggregate);
    }

    #[test]
    fn merge_single_profile_failure_returns_err() {
        let per_profile: Vec<(String, anyhow::Result<QueryGenericResponse>)> =
            vec![("prod".to_string(), Err(anyhow::anyhow!("boom")))];
        let result = merge_results(per_profile, false, None);

        assert!(result.is_err(), "single failing profile must bail");
    }

    #[test]
    fn merge_all_profiles_failing_returns_err() {
        let per_profile: Vec<(String, anyhow::Result<QueryGenericResponse>)> = vec![
            ("prod".to_string(), Err(anyhow::anyhow!("timeout"))),
            ("staging".to_string(), Err(anyhow::anyhow!("auth failed"))),
        ];
        let result = merge_results(per_profile, true, None);

        assert!(
            result.is_err(),
            "must bail when every profile in the fan-out failed"
        );
    }

    // ── check_cases_query_rules integration via merge_results ────────────────

    use crate::cases_query_rules::check_cases_query_rules;

    const CASES_START: &str = "2024-01-01T00:00:00.000Z";
    const CASES_END: &str = "2024-01-01T02:00:00.000Z";
    const CASES_QUERY: &str = "source system/labs.cases.state_updates | count";

    #[test]
    fn cases_warning_prepended_to_merged_warnings() {
        let warning = check_cases_query_rules(CASES_QUERY, CASES_START, CASES_END);
        assert!(
            warning.is_some(),
            "expected a warning from check_cases_query_rules"
        );

        let per_profile = vec![(
            "prod".to_string(),
            Ok(make_generic_response(vec![json!({"data": 1})], false)),
        )];
        let merged = merge_results(per_profile, false, warning.as_deref()).unwrap();

        assert_eq!(merged.warnings.len(), 1);
        assert!(
            merged.warnings[0].contains("[Cases query warning]"),
            "cases warning should be in merged warnings"
        );
    }

    #[test]
    fn cases_warning_plus_profile_warning_both_appear() {
        let cases_warn = check_cases_query_rules(CASES_QUERY, CASES_START, CASES_END);
        assert!(cases_warn.is_some());

        let mut resp = make_generic_response(vec![], false);
        resp.warnings = vec!["too many results".to_string()];
        let per_profile = vec![("prod".to_string(), Ok(resp))];
        let merged = merge_results(per_profile, true, cases_warn.as_deref()).unwrap();

        assert_eq!(
            merged.warnings.len(),
            2,
            "should have one cases warning and one profile warning"
        );
        assert!(merged.warnings[0].contains("[Cases query warning]"));
        assert!(merged.warnings[1].contains("[prod]"));
        assert!(merged.warnings[1].contains("too many results"));
    }

    #[test]
    fn cases_warning_present_when_profile_errors() {
        let cases_warn = check_cases_query_rules(CASES_QUERY, CASES_START, CASES_END);
        assert!(cases_warn.is_some());
        let warning_text = cases_warn.clone().unwrap();

        // With a cases warning and a failed profile, the warning is still in
        // merged.warnings (printed by render_results), not embedded in errors.
        let per_profile: Vec<(String, anyhow::Result<QueryGenericResponse>)> = vec![
            (
                "good".to_string(),
                Ok(make_generic_response(vec![json!({"ok": true})], false)),
            ),
            ("bad".to_string(), Err(anyhow::anyhow!("timeout"))),
        ];
        let merged = merge_results(per_profile, true, cases_warn.as_deref()).unwrap();

        // The cases warning appears in merged.warnings regardless of profile errors.
        assert!(
            merged
                .warnings
                .iter()
                .any(|w| w.contains("[Cases query warning]")),
            "cases warning must be present even when a profile errors"
        );
        // The good profile's row is still included.
        assert_eq!(merged.rows.len(), 1);
        // The warning text is non-empty (contains Rule content).
        assert!(warning_text.contains("Rule"));
    }

    #[test]
    fn no_cases_warning_when_query_has_dedup() {
        let warning = check_cases_query_rules(
            "source system/labs.cases.state_updates | dedupeby caseId orderby $m.timestamp desc",
            CASES_START,
            CASES_END,
        );
        let per_profile = vec![("prod".to_string(), Ok(make_generic_response(vec![], false)))];
        let merged = merge_results(per_profile, false, warning.as_deref()).unwrap();

        assert!(
            merged.warnings.is_empty(),
            "no warnings expected for a compliant cases query"
        );
    }

    #[test]
    fn no_cases_warning_for_non_cases_source() {
        let warning = check_cases_query_rules("source logs | limit 10", CASES_START, CASES_END);
        assert!(warning.is_none());

        let per_profile = vec![(
            "prod".to_string(),
            Ok(make_generic_response(vec![json!({"msg": "hi"})], false)),
        )];
        let merged = merge_results(per_profile, false, warning.as_deref()).unwrap();

        assert!(merged.warnings.is_empty());
        assert_eq!(merged.rows.len(), 1);
    }
}
