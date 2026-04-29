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

use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
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
    start: &str,
    end: &str,
    limit: u32,
    tier: Tier,
    source: &str,
) -> Result<QueryGenericResponse> {
    let api = DataprimeApi::new(&target.client);
    let start_ts = parse_timestamp(start)?;
    let end_ts = parse_timestamp(end)?;
    Ok(api
        .query_generic(query, &start_ts, &end_ts, limit, tier, source)
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
) -> MergedResults {
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

    MergedResults {
        rows,
        warnings,
        is_aggregate: is_aggregate.unwrap_or(false),
        include_profile,
    }
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
    tier: Tier,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
    text_renderer: Option<fn(&MergedResults) -> Result<()>>,
) -> Result<()> {
    eprintln!("{}", "Querying...".dimmed());

    let include_profile = targets.len() > 1;
    let query = query.to_string();
    let source = source.to_string();
    let start = start.to_string();
    let end = end.to_string();
    let per_profile = fan_out(targets, |t| {
        let q = query.clone();
        let src = source.clone();
        let s = start.clone();
        let e = end.clone();
        async move { execute_query(t, &q, &s, &e, limit, tier, &src).await }
    })
    .await;

    let merged = merge_results(per_profile, include_profile);
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
        let merged = merge_results(per_profile, false);

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
        let merged = merge_results(per_profile, true);

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
        let merged = merge_results(per_profile, true);

        assert_eq!(merged.rows.len(), 1);
        assert_eq!(merged.rows[0]["profile"], json!("good"));
    }

    #[test]
    fn merge_collects_warnings_with_profile_prefix() {
        let mut resp = make_generic_response(vec![], false);
        resp.warnings = vec!["too many results".to_string()];
        let per_profile = vec![("prod".to_string(), Ok(resp))];
        let merged = merge_results(per_profile, true);

        assert_eq!(merged.warnings.len(), 1);
        assert!(merged.warnings[0].contains("[prod]"));
    }

    #[test]
    fn merge_is_aggregate_from_first_successful() {
        let per_profile = vec![
            ("p1".to_string(), Ok(make_generic_response(vec![], true))),
            ("p2".to_string(), Ok(make_generic_response(vec![], true))),
        ];
        let merged = merge_results(per_profile, true);
        assert!(merged.is_aggregate);
    }
}
