//! Shared rendering helpers for command output.
//!
//! Provides functions for the three output modes (Text, JSON, Agents) so that
//! individual commands can delegate formatting without duplicating boilerplate.
//!
//! ## Profile key convention
//!
//! List commands inject a `"profile"` key into JSON rows inline while merging
//! `execution::report_errors_and_collect_successes` results. Get commands use `"_profile"`
//! (underscore-prefixed) as a rendering hint that should not appear in
//! user-facing JSON output - see [`tag_get_result`].

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use tabled::builder::Builder;
use toon_format::encode_default as toon_encode;
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

/// Format merged result rows for agents output (TOON-encoded).
///
/// See `docs/agents-output.md` — agents mode uses TOON, not pretty JSON.
pub fn format_agents(rows: &[Value]) -> Result<String> {
    let wrapped = Value::Array(rows.to_vec());
    toon_encode(&wrapped).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))
}

/// Print merged rows in agents (TOON) format.
pub fn render_agents(rows: &[Value]) -> Result<()> {
    println!("{}", format_agents(rows)?);
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

/// Print a create/update success line to stderr.
///
/// Prints green with the ID when the API response included one. Otherwise
/// prints a yellow warning rather than fabricating a placeholder ID - a
/// missing ID from an otherwise-successful response is unusual enough to
/// flag, not paper over.
///
/// `name` is `None` for resources that have no display name of their own
/// (e.g. a bare integration or role ID).
pub fn print_created(verb: &str, kind: &str, name: Option<&str>, id: Option<&str>, profile: &str) {
    let subject = match name {
        Some(name) => format!("{kind} '{name}'"),
        None => kind.to_string(),
    };
    match id {
        Some(id) => eprintln!(
            "{}",
            format!("{verb} {subject} (ID: {id}) in profile '{profile}'.").green()
        ),
        None => eprintln!(
            "{}",
            format!(
                "{verb} {subject} in profile '{profile}', but the response did not include an ID."
            )
            .yellow()
        ),
    }
}

/// Print a "View in Coralogix" console link to stderr.
///
/// Callers only invoke this when a console base URL was successfully
/// resolved (see `ExecutionTarget::console_base`) and an entity ID was
/// extracted from the API response. This is the human-readable echo of the
/// same URL that `tag_console_url` embeds in `-o json` / `-o agents`
/// payloads - the two are always called together at each call site so the
/// stderr line and the structured `consoleUrl` field never disagree.
pub fn print_console_link(url: &str) {
    eprintln!("{}", format!("View in Coralogix: {url}").cyan());
}

/// Print a hint explaining why no "View in Coralogix" link is available for
/// a profile.
///
/// Callers invoke this from inside `ExecutionTarget::console_base`'s
/// `OnceCell::get_or_init`, so it prints at most once per target per
/// process, not once per command that would have printed a link
/// (`console_base` is cached, so repeated lookups within one invocation
/// reuse the cached `None` rather than re-triggering this hint).
///
/// This is reached whenever neither `console_url` nor `console_team_name`
/// (combined with a known `console_domain` for the profile's region) is
/// configured. `cx` never calls any API or guesses a team's console
/// subdomain - both fields are purely user-supplied (see `src/config.rs`'s
/// `Profile` struct), so the only fix is to set one of them explicitly.
pub fn print_console_link_unavailable_hint() {
    eprintln!(
        "{}",
        "Note: no \"View in Coralogix\" link available for this profile (no `console_url` or \
         `console_team_name` configured, or the region has no known console domain). Set \
         `console_url` in the profile's TOML for a full override, or `console_team_name` to \
         build one from the region's console domain - see docs/configuration.md#console-links."
            .dimmed()
    );
}

/// Embed the "View in Coralogix" URL as a `consoleUrl` field directly in a
/// JSON result object, so `-o json` / `-o agents` consumers get the link in
/// the structured payload itself, not only as an informational stderr line.
///
/// Uses a plain (non-underscore-prefixed) key, since - unlike `_profile`,
/// which is purely a rendering aid for multi-profile text mode -
/// `consoleUrl` is meant to be consumed by callers of `-o json`/`-o agents`
/// as real, documented output data (see `docs/configuration.md`'s
/// "Console links" section). No-op if `val` isn't a JSON object (e.g. a
/// bare scalar/array result), so it's always safe to call unconditionally
/// alongside `print_console_link`.
pub fn tag_console_url(val: &mut Value, url: &str) {
    if let Value::Object(ref mut m) = val {
        m.insert("consoleUrl".to_string(), Value::String(url.to_string()));
    }
}

// ── Text tables ──────────────────────────────────────────────────────────────

/// Build a text table string with an optional "Profile" column.
///
/// `headers` should **not** include "Profile" - it is prepended automatically
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
    fn format_agents_toon_differs_from_pretty_json() {
        let rows = vec![json!({"query_text": "q", "similarity": 0.5})];
        let json = format_json(&rows).unwrap();
        let agents = format_agents(&rows).unwrap();
        assert_ne!(
            json, agents,
            "agents output must be TOON, not identical to pretty JSON"
        );
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
