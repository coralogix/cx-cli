//! Shared rendering helpers for command output.
//!
//! Provides functions for the three output modes (Text, JSON, Agents) so that
//! individual commands can delegate formatting without duplicating boilerplate.
//!
//! ## Profile key convention
//!
//! List commands inject a `"profile"` key into JSON rows via `execution::tag_rows`.
//! Get commands use `"_profile"` (underscore-prefixed) as a rendering hint that
//! should not appear in user-facing JSON output — see [`tag_get_result`].

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use tabled::builder::Builder;
// ── JSON output ──────────────────────────────────────────────────────────────

/// Format rows as a JSON array.
pub fn format_json(rows: &[Value]) -> Result<String> {
    Ok(serde_json::to_string_pretty(rows)?)
}

/// Render rows as a JSON array (for list commands).
pub fn render_json(rows: &[Value]) -> Result<()> {
    println!("{}", format_json(rows)?);
    Ok(())
}

/// Format JSON, unwrapping single-element arrays to a bare object.
pub fn format_json_auto(rows: &[Value]) -> Result<String> {
    if rows.len() == 1 {
        Ok(serde_json::to_string_pretty(&rows[0])?)
    } else {
        Ok(serde_json::to_string_pretty(rows)?)
    }
}

/// Render JSON, unwrapping single-element arrays (for get commands).
pub fn render_json_auto(rows: &[Value]) -> Result<()> {
    println!("{}", format_json_auto(rows)?);
    Ok(())
}

// ── Text output helpers ──────────────────────────────────────────────────────

/// Print a "no results" message in yellow.
pub fn print_no_results(msg: &str) {
    println!("{}", msg.yellow());
}

/// Display `Option<bool>` as "yes" / "no" / "-".
pub fn bool_display(v: Option<bool>) -> String {
    match v {
        Some(true) => "yes".to_string(),
        Some(false) => "no".to_string(),
        None => "-".to_string(),
    }
}

// ── Text tables ──────────────────────────────────────────────────────────────

/// Build a text table string with an optional "Profile" column.
///
/// `headers` should **not** include "Profile" — it is prepended automatically
/// when `include_profile` is true.
///
/// Each element of `rows` is a `Vec<String>` whose **first** element is the
/// profile name.  When `include_profile` is false, that first element is
/// silently skipped.
pub fn format_table(headers: &[&str], rows: Vec<Vec<String>>, include_profile: bool) -> String {
    let mut builder = Builder::default();

    let header: Vec<String> = if include_profile {
        std::iter::once("Profile".to_string())
            .chain(headers.iter().map(|h| h.to_string()))
            .collect()
    } else {
        headers.iter().map(|h| h.to_string()).collect()
    };
    builder.push_record(header);

    for row in rows {
        let record: Vec<String> = if include_profile {
            row
        } else {
            row.into_iter().skip(1).collect()
        };
        builder.push_record(record);
    }

    builder.build().to_string()
}

/// Render a text table with an optional "Profile" column.
///
/// See [`format_table`] for details on the `headers` / `rows` contract.
pub fn render_table(headers: &[&str], rows: Vec<Vec<String>>, include_profile: bool) {
    println!("{}", format_table(headers, rows, include_profile));
}

// ── "Get" command text rendering ─────────────────────────────────────────────

/// Tag a JSON value with `_profile` for get-command text rendering.
///
/// Uses an underscore-prefixed key so it doesn't collide with real data fields.
pub fn tag_get_result(val: &mut Value, profile: &str) {
    if let Value::Object(ref mut m) = val {
        m.insert("_profile".to_string(), Value::String(profile.to_string()));
    }
}

/// Render "get" command results in text mode.
///
/// For each result:
/// 1. Optionally prints a dimmed `[profile_name]` header
/// 2. Calls `summary_fn` (if provided) to print domain-specific key-value lines
/// 3. Prints the full pretty-printed JSON body
pub fn render_get_text(
    results: &[Value],
    include_profile: bool,
    empty_msg: &str,
    summary_fn: Option<&dyn Fn(&Value)>,
) -> Result<()> {
    if results.is_empty() {
        print_no_results(empty_msg);
        return Ok(());
    }
    for val in results {
        if include_profile {
            if let Some(p) = val.get("_profile").and_then(|v| v.as_str()) {
                println!("{}", format!("[{p}]").dimmed());
            }
        }
        if let Some(f) = summary_fn {
            f(val);
            println!();
        }
        println!("{}", serde_json::to_string_pretty(val)?);
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bool_display_some_true() {
        assert_eq!(bool_display(Some(true)), "yes");
    }

    #[test]
    fn bool_display_some_false() {
        assert_eq!(bool_display(Some(false)), "no");
    }

    #[test]
    fn bool_display_none() {
        assert_eq!(bool_display(None), "-");
    }

    #[test]
    fn format_table_with_profile() {
        let headers = &["Name", "Value"];
        let rows = vec![
            vec!["prod".into(), "foo".into(), "42".into()],
            vec!["staging".into(), "bar".into(), "99".into()],
        ];
        let output = format_table(headers, rows, true);
        assert!(output.contains("Profile"));
        assert!(output.contains("Name"));
        assert!(output.contains("Value"));
        assert!(output.contains("prod"));
        assert!(output.contains("staging"));
        assert!(output.contains("foo"));
        assert!(output.contains("42"));
    }

    #[test]
    fn format_table_without_profile() {
        let headers = &["Name", "Value"];
        let rows = vec![
            vec!["prod".into(), "foo".into(), "42".into()],
            vec!["staging".into(), "bar".into(), "99".into()],
        ];
        let output = format_table(headers, rows, false);
        assert!(!output.contains("Profile"));
        assert!(!output.contains("prod"));
        assert!(!output.contains("staging"));
        assert!(output.contains("Name"));
        assert!(output.contains("foo"));
        assert!(output.contains("42"));
    }

    #[test]
    fn format_json_auto_unwraps_single() {
        let rows = vec![json!({"id": "abc"})];
        let output = format_json_auto(&rows).unwrap();
        // Should be a bare object, not an array
        assert!(output.starts_with('{'));
        assert!(output.contains("\"abc\""));
    }

    #[test]
    fn format_json_auto_keeps_array_for_multiple() {
        let rows = vec![json!({"id": "a"}), json!({"id": "b"})];
        let output = format_json_auto(&rows).unwrap();
        // Should be an array
        assert!(output.starts_with('['));
    }

    #[test]
    fn format_json_renders_array() {
        let rows = vec![json!({"x": 1})];
        let output = format_json(&rows).unwrap();
        assert!(output.starts_with('['));
    }

    #[test]
    fn tag_get_result_inserts_underscore_profile() {
        let mut val = json!({"name": "test"});
        tag_get_result(&mut val, "prod");
        assert_eq!(val["_profile"], json!("prod"));
    }

    #[test]
    fn tag_get_result_noop_on_non_object() {
        let mut val = json!("plain string");
        tag_get_result(&mut val, "prod");
        assert_eq!(val, json!("plain string"));
    }
}
