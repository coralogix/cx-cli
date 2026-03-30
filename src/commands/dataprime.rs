use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use tabled::{Table, Tabled};
use toon_format::encode_default as toon_encode;

use crate::config::{self, OutputFormat};

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

/// Returns the path to the dataprime docs YAML file.
pub fn docs_file() -> Result<PathBuf> {
    Ok(config::config_dir()?.join("dataprime_docs.yaml"))
}

/// Load the DataPrime documentation from disk.
pub fn load_docs() -> Result<DataprimeDocs> {
    let path = docs_file()?;
    let content = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "DataPrime documentation not found at {}.\n\n\
            Run the following to generate it:\n  \
            python3 scripts/generate_dataprime_docs.py --input <path-to-dataprime_docs.json>\n\n\
            The docs will be written to: {}",
            path.display(),
            path.display()
        )
    })?;
    serde_yaml::from_str(&content).context("Failed to parse dataprime_docs.yaml")
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
    } else if s.ends_with('.') {
        s.lines().next().unwrap_or(s).to_string()
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
        let all_names: Vec<_> = docs
            .commands
            .keys()
            .chain(docs.functions.keys())
            .collect();

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
        assert_eq!(
            first_sentence("First line\nSecond line"),
            "First line"
        );
    }
}
