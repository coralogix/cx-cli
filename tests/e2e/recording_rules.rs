use std::sync::OnceLock;

use crate::harness;
use serde_json::{json, Value};

#[test]
#[ignore]
fn recording_rules_list() {
    if harness::require_creds("recording_rules_list").is_none() {
        return;
    }
    // The writable e2e team starts with no recording rule sets; the
    // lifecycle test below creates its own, so only the shape is asserted.
    let v = harness::run_ok_json(&["recording-rules", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name", "rules_count"]);
}

#[test]
#[ignore]
fn recording_rules_get() {
    if harness::require_creds("recording_rules_get").is_none() {
        return;
    }
    let id = match discover_recording_rule_id() {
        Some(id) => id,
        None => {
            eprintln!(
                "[e2e] skipping recording_rules_get: no recording rules available on test team"
            );
            return;
        }
    };
    let v = harness::run_ok_json(&["recording-rules", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

fn discover_recording_rule_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("recording_rules_discover")?;
            let stdout = harness::run_ok(&["recording-rules", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string().trim_matches('"').to_string(),
                })
        })
        .clone()
}

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

/// A set with one group and one rule. The expression is a constant, so it
/// never reads live metrics, and the record name is unique to this run.
fn set_body(name: &str, expr: &str) -> Value {
    json!({
        "name": name,
        "groups": [
            {
                "name": name,
                "interval": 60,
                "rules": [ { "record": name.replace('-', "_"), "expr": expr } ]
            }
        ]
    })
}

/// `get` returns the set at the top level: `{id, name, groups: [...]}`.
fn first_expr(get_response: &Value) -> Option<&str> {
    get_response
        .get("group")
        .unwrap_or(get_response)
        .pointer("/groups/0/rules/0/expr")
        .and_then(Value::as_str)
}

#[test]
#[ignore]
fn recording_rules_lifecycle() {
    if harness::require_write_creds("recording_rules_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("rr");

    // create: the API answers only `{"id"}`; the CLI reads the set back
    let file = harness::JsonFile::new("rr", &set_body(&name, "vector(1)"));
    let created = harness::run_write_json(&[
        "recording-rules",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("recording rule create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["recording-rules", "delete", &id]);
    assert_eq!(created["name"], name, "create should report our name");
    assert_eq!(
        created["rules_count"],
        json!(1),
        "create should count the rule"
    );

    // get
    let got = harness::run_ok_json(&["recording-rules", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name"]);
    assert_eq!(got["name"], name);
    assert_eq!(first_expr(&got), Some("vector(1)"), "{got}");

    // list contains it
    let list = harness::run_ok_json(&["recording-rules", "list", "-o", "json"]);
    let ours = harness::find_by_name(&list, "name", &name)
        .unwrap_or_else(|| panic!("recording rule list should contain {name}"));
    assert_eq!(ours["rules_count"], json!(1));
    assert_eq!(ours["interval"], json!("60"));

    // update: change the expression
    let upd = harness::JsonFile::new("rr-upd", &set_body(&name, "vector(2)"));
    let updated = harness::run_write_json(&[
        "recording-rules",
        "update",
        "--from-file",
        upd.path(),
        &id,
        "-o",
        "json",
    ]);
    assert_eq!(
        updated["id"],
        json!(id),
        "update should report the set id: {updated}"
    );
    let got = harness::run_ok_json(&["recording-rules", "get", &id, "-o", "json"]);
    assert_eq!(
        first_expr(&got),
        Some("vector(2)"),
        "update should change the expression: {got}"
    );

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["recording-rules", "get", &id]);
    harness::run_ok_nonempty(&["recording-rules", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["recording-rules", "list"]);
    harness::run_ok_nonempty(&["recording-rules", "list", "-o", "toon"]);

    // delete, then get must fail
    harness::run_write_ok(&["recording-rules", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["recording-rules", "get", &id, "-o", "json"]);
}

#[test]
#[ignore]
fn recording_rules_get_and_delete_missing_fail() {
    if harness::require_write_creds("recording_rules_get_and_delete_missing_fail").is_none() {
        return;
    }
    let bogus = "00000000000000000000000000";
    harness::run_fail(&["recording-rules", "get", bogus, "-o", "json"]);
    harness::run_fail(&["recording-rules", "delete", bogus, "--yes"]);
}
