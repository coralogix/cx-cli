use std::io::Read;
use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::Value;

use super::api::{OllyApi, RuleScope};
use crate::config::OutputFormat;
use crate::error::CxError;
use crate::execution::ExecutionTarget;
use crate::render;

/// Longest rule text shown in the `list` table. JSON output is never cut.
const TABLE_RULE_CHARS: usize = 80;

/// Rules belong to one user or team, so `cx olly rules` takes one profile.
fn single_api(targets: &[Arc<ExecutionTarget>]) -> Result<OllyApi> {
    if targets.len() > 1 {
        bail!("cx olly does not support multi-profile. Use a single profile with -p <profile>.");
    }
    Ok(OllyApi::from_client(targets[0].client.clone()))
}

/// Personal rules belong to a user, so the API refuses a team API key with
/// "A user identity is required. Team API keys are not supported for this
/// endpoint." Add the way out to that answer, and leave other 403s alone.
fn explain(scope: RuleScope, err: CxError) -> CxError {
    match (scope, err) {
        (RuleScope::Personal, CxError::Permission(detail))
            if detail.contains("user identity is required") || detail.contains("Team API keys") =>
        {
            CxError::Permission(format!(
                "{detail}\nPersonal rules need a personal API key or a logged-in profile. \
             A team API key cannot use them; pass --team to work with team rules."
            ))
        }
        (_, err) => err,
    }
}

/// The text of `--rule`. `-` reads it from stdin.
pub fn read_rule_text(raw: &str) -> Result<String> {
    let text = if raw == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        raw.to_string()
    };
    let text = text.trim();
    if text.is_empty() {
        bail!("The rule text is empty. Pass it with --rule <text>, or --rule - to read stdin.");
    }
    Ok(text.to_string())
}

fn str_field<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("-")
}

fn enabled(row: &Value) -> String {
    render::bool_display(row.get("enabled").and_then(Value::as_bool))
}

/// One line, at most [`TABLE_RULE_CHARS`] characters.
fn table_rule_text(rule: &str) -> String {
    let line = rule.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= TABLE_RULE_CHARS {
        return line;
    }
    let cut: String = line.chars().take(TABLE_RULE_CHARS - 1).collect();
    format!("{cut}…")
}

/// Print one rule: the whole object for JSON and TOON, labelled lines and the
/// full text for humans.
fn render_rule(rule: Value, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json_auto(&[rule]),
        OutputFormat::Toon => render::render_toon(&[rule]),
        OutputFormat::Text => {
            println!("ID: {}", str_field(&rule, "id"));
            println!("Enabled: {}", enabled(&rule));
            println!("Created: {}", str_field(&rule, "created_at"));
            println!("\n{}", str_field(&rule, "rule"));
            Ok(())
        }
    }
}

/// After a write, JSON and TOON get the rule the API returned. Text mode has
/// already printed the status line.
fn render_written(rule: Value, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Text => Ok(()),
        _ => render_rule(rule, output),
    }
}

pub async fn run_rules_list(
    targets: &[Arc<ExecutionTarget>],
    scope: RuleScope,
    output: OutputFormat,
) -> Result<()> {
    let api = single_api(targets)?;
    eprintln!(
        "{}",
        format!("Fetching {} rules...", scope.label()).dimmed()
    );
    let rules = api.list_rules(scope).await.map_err(|e| explain(scope, e))?;

    match output {
        OutputFormat::Json => render::render_json(&rules)?,
        OutputFormat::Toon => render::render_toon(&rules)?,
        OutputFormat::Text => {
            if rules.is_empty() {
                render::print_no_results(&format!("No {} rules found.", scope.label()));
                return Ok(());
            }
            let rows = rules
                .iter()
                .map(|rule| {
                    // `render_table` expects the profile name first and drops
                    // it for a single profile.
                    vec![
                        String::new(),
                        str_field(rule, "id").to_string(),
                        enabled(rule),
                        str_field(rule, "created_at").to_string(),
                        table_rule_text(str_field(rule, "rule")),
                    ]
                })
                .collect();
            render::render_table(&["ID", "ENABLED", "CREATED", "RULE"], rows, false);
        }
    }
    Ok(())
}

pub async fn run_rules_get(
    targets: &[Arc<ExecutionTarget>],
    scope: RuleScope,
    rule_id: &str,
    output: OutputFormat,
) -> Result<()> {
    let api = single_api(targets)?;
    let rule = api
        .get_rule(scope, rule_id)
        .await
        .map_err(|e| explain(scope, e))?;
    render_rule(rule, output)
}

pub async fn run_rules_create(
    targets: &[Arc<ExecutionTarget>],
    scope: RuleScope,
    text: &str,
    output: OutputFormat,
) -> Result<()> {
    let api = single_api(targets)?;
    let rule = api
        .create_rule(scope, text)
        .await
        .map_err(|e| explain(scope, e))?;
    eprintln!(
        "{}",
        format!("Created {} rule {}.", scope.label(), str_field(&rule, "id")).green()
    );
    render_written(rule, output)
}

/// `update` passes the new text; `enable` and `disable` pass `enabled`.
pub async fn run_rules_update(
    targets: &[Arc<ExecutionTarget>],
    scope: RuleScope,
    rule_id: &str,
    text: Option<&str>,
    enabled: Option<bool>,
    output: OutputFormat,
) -> Result<()> {
    let api = single_api(targets)?;
    let rule = api
        .update_rule(scope, rule_id, text, enabled)
        .await
        .map_err(|e| explain(scope, e))?;
    let verb = match enabled {
        Some(true) => "Enabled",
        Some(false) => "Disabled",
        None => "Updated",
    };
    eprintln!(
        "{}",
        format!("{verb} {} rule {rule_id}.", scope.label()).green()
    );
    render_written(rule, output)
}

pub async fn run_rules_delete(
    targets: &[Arc<ExecutionTarget>],
    scope: RuleScope,
    rule_id: &str,
) -> Result<()> {
    single_api(targets)?
        .delete_rule(scope, rule_id)
        .await
        .map_err(|e| explain(scope, e))?;
    eprintln!(
        "{}",
        format!("Deleted {} rule {rule_id}.", scope.label()).green()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_text_is_one_line_and_cut_at_the_limit() {
        assert_eq!(table_rule_text("Answer\nin  English"), "Answer in English");
        let long = "é".repeat(200);
        let cut = table_rule_text(&long);
        assert_eq!(cut.chars().count(), TABLE_RULE_CHARS);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn rule_text_is_trimmed_and_blank_is_rejected() {
        assert_eq!(read_rule_text("  be brief \n").unwrap(), "be brief");
        assert!(read_rule_text("   ").is_err());
    }

    #[test]
    fn personal_permission_errors_name_the_key_type() {
        let team_key =
            "A user identity is required. Team API keys are not supported for this endpoint.";
        let personal = explain(RuleScope::Personal, CxError::Permission(team_key.into()));
        assert!(personal.to_string().contains("personal API key"));
        let other = explain(RuleScope::Personal, CxError::Permission("Forbidden".into()));
        assert_eq!(other.to_string(), "Permission denied: Forbidden");
        let team = explain(RuleScope::Team, CxError::Permission(team_key.into()));
        assert_eq!(team.to_string(), format!("Permission denied: {team_key}"));
    }
}
