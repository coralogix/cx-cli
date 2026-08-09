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

/// Embed the "View in Coralogix" URL as a `consoleUrl` field in a JSON
/// result object, so `-o json` / `-o agents` consumers get the link in the
/// structured payload itself, not only as an informational stderr line.
///
/// Many `cx` command responses wrap their single "real" entity in an object
/// with exactly one key, mirroring the underlying API's response shape -
/// e.g. `{"alertDef": {...}}`, `{"case": {...}}`, `{"webhook": {...}}`.
/// Inserting `consoleUrl` at the root of a response shaped like that would
/// make it an odd sibling of the wrapper key instead of living alongside
/// the entity's own fields (`{"alertDef": {...}, "consoleUrl": ...}` reads
/// oddly - a consumer expecting `alertDef` to be the complete alert object
/// has to know to look one level up for its console link). So: when `val`
/// is a JSON object with **exactly one** key whose value is *itself* a JSON
/// object (ignoring any `_profile` tag), `consoleUrl` is inserted into that
/// nested object instead of the root. This was checked against every response
/// shape exercised by
/// `tests/console_urls/main.rs` (spanning all 28 command groups that print
/// console links) and correctly nests every single-entity-wrapper shape
/// while leaving every other shape at the root:
/// - Already-flat entities with multiple top-level fields (e.g. a `connectors get`
///   response with `id`/`name`/`type` at the root) - nothing to descend into.
/// - Single-key responses whose value isn't an object - a bare id/count/bool
///   (`{"id": "..."}`, `{"enabled": true}`) or a list (`{"policies": [...]}`,
///   `{"enrichments": [...]}`) - again nothing to nest a single console link
///   into.
///
/// No-op if `val` isn't a JSON object at all (e.g. a bare scalar/array
/// result), so it's always safe to call unconditionally alongside
/// `print_console_link`.
///
/// Also a no-op if there's no real data to attach the link to in the first
/// place - an empty root (ignoring `_profile`) or an empty nested wrapper,
/// which some backends return on an otherwise-successful write. Tagging
/// either would produce a JSON object whose *only* content is the URL that
/// `print_console_link` already echoed to stderr moments earlier, which
/// duplicates rather than adds information.
pub fn tag_console_url(val: &mut Value, url: &str) {
    let Value::Object(root) = val else { return };

    // In multi-profile mode `tag_get_result` may have already inserted a
    // `_profile` tag at the root. That's presentation metadata, not part of the
    // entity, so ignore it when deciding whether this is a single-object
    // wrapper to nest into. Otherwise the wrapper would look like a flat
    // multi-field object and the link would land at a *different* JSON path in
    // multi-profile vs. single-profile output, breaking scripts parsing it.
    let nest_key = {
        let mut data_keys = root.iter().filter(|(k, _)| k.as_str() != "_profile");
        match (data_keys.next(), data_keys.next()) {
            (Some((k, Value::Object(_))), None) => Some(k.clone()),
            _ => None,
        }
    };

    if let Some(key) = nest_key {
        if let Some(Value::Object(nested)) = root.get_mut(&key) {
            if nested.is_empty() {
                return;
            }
            nested.insert("consoleUrl".to_string(), Value::String(url.to_string()));
            return;
        }
    }

    if root.iter().all(|(k, _)| k == "_profile") {
        return;
    }

    root.insert("consoleUrl".to_string(), Value::String(url.to_string()));
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
    fn tag_console_url_nests_inside_single_object_wrapper() {
        let mut val = json!({"alertDef": {"id": "a1", "name": "Demo"}});
        tag_console_url(&mut val, "https://acme.app.eu2.coralogix.com/#/alerts/a1");
        assert_eq!(val["consoleUrl"], Value::Null, "must not tag the root");
        assert_eq!(
            val["alertDef"]["consoleUrl"],
            "https://acme.app.eu2.coralogix.com/#/alerts/a1"
        );
    }

    #[test]
    fn tag_console_url_nests_inside_wrapper_even_with_profile_tag() {
        // Multi-profile mode: `tag_get_result` adds `_profile` at the root
        // first. The link must still nest inside the wrapper so it lands at the
        // same path (`.alertDef.consoleUrl`) as single-profile output.
        let mut val = json!({"alertDef": {"id": "a1", "name": "Demo"}, "_profile": "prod"});
        tag_console_url(&mut val, "https://acme.app.eu2.coralogix.com/#/alerts/a1");
        assert_eq!(val["consoleUrl"], Value::Null, "must not tag the root");
        assert_eq!(
            val["alertDef"]["consoleUrl"],
            "https://acme.app.eu2.coralogix.com/#/alerts/a1"
        );
        assert_eq!(val["_profile"], "prod", "profile tag must be preserved");
    }

    #[test]
    fn tag_console_url_stays_at_root_for_flat_entity_with_profile_tag() {
        // Already-flat entity + `_profile`: still no single object wrapper to
        // descend into, so the link stays at the root.
        let mut val = json!({"id": "conn-1", "name": "Demo", "type": "SLACK", "_profile": "prod"});
        tag_console_url(&mut val, "https://acme.app.eu2.coralogix.com/#/x");
        assert_eq!(val["consoleUrl"], "https://acme.app.eu2.coralogix.com/#/x");
    }

    #[test]
    fn tag_console_url_stays_at_root_for_already_flat_multi_field_object() {
        let mut val = json!({"id": "conn-1", "name": "Demo Connector", "type": "SLACK"});
        tag_console_url(&mut val, "https://acme.app.eu2.coralogix.com/#/x");
        assert_eq!(val["consoleUrl"], "https://acme.app.eu2.coralogix.com/#/x");
    }

    #[test]
    fn tag_console_url_stays_at_root_for_single_key_string_value() {
        let mut val = json!({"dashboardId": "dash-1"});
        tag_console_url(
            &mut val,
            "https://acme.app.eu2.coralogix.com/#/dashboards/dash-1",
        );
        assert_eq!(
            val["consoleUrl"],
            "https://acme.app.eu2.coralogix.com/#/dashboards/dash-1"
        );
    }

    #[test]
    fn tag_console_url_stays_at_root_for_single_key_array_value() {
        let mut val = json!({"policies": []});
        tag_console_url(
            &mut val,
            "https://acme.app.eu2.coralogix.com/#/tco-policies",
        );
        assert_eq!(
            val["consoleUrl"],
            "https://acme.app.eu2.coralogix.com/#/tco-policies"
        );
    }

    #[test]
    fn tag_console_url_no_op_on_non_object_value() {
        let mut val = json!([1, 2, 3]);
        tag_console_url(&mut val, "https://acme.app.eu2.coralogix.com/#/x");
        assert_eq!(val, json!([1, 2, 3]));
    }

    #[test]
    fn tag_console_url_no_op_on_completely_empty_object() {
        // Some backends echo an empty `{}` on an otherwise-successful write.
        // Tagging it would make stdout's only content a duplicate of the
        // link `print_console_link` already printed to stderr.
        let mut val = json!({});
        tag_console_url(&mut val, "https://acme.app.eu2.coralogix.com/#/x");
        assert_eq!(val, json!({}));
    }

    #[test]
    fn tag_console_url_no_op_on_profile_tag_only_object() {
        let mut val = json!({"_profile": "prod"});
        tag_console_url(&mut val, "https://acme.app.eu2.coralogix.com/#/x");
        assert_eq!(val, json!({"_profile": "prod"}));
    }

    #[test]
    fn tag_console_url_no_op_on_empty_nested_wrapper() {
        let mut val = json!({"alertDef": {}});
        tag_console_url(&mut val, "https://acme.app.eu2.coralogix.com/#/x");
        assert_eq!(val, json!({"alertDef": {}}));
    }

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
