//! `cx retentions` e2e. Retention tags are a team-wide singleton; the update
//! test writes the current names back unchanged.

use crate::harness;
use serde_json::{json, Value};

fn retentions() -> Value {
    let v = harness::run_ok_json(&["retentions", "list", "-o", "json"]);
    // Response may be a flat array or wrapped in {"retentions": [...]}
    let arr = if v.is_array() {
        v
    } else {
        v.get("retentions").cloned().unwrap_or(v)
    };
    harness::assert_nonempty_array_of_objects_with_keys(&arr, &["id", "name", "order"]);
    arr
}

#[test]
#[ignore]
fn retentions_list() {
    if harness::require_creds("retentions_list").is_none() {
        return;
    }
    retentions();
    harness::run_ok_nonempty(&["retentions", "list"]);
    harness::run_ok_nonempty(&["retentions", "list", "-o", "toon"]);
}

#[test]
#[ignore]
fn retentions_status() {
    if harness::require_creds("retentions_status").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["retentions", "status", "-o", "json"]);
    assert!(v.is_object(), "status should be a JSON object: {v}");
    harness::run_ok_nonempty(&["retentions", "status"]);
}

#[test]
#[ignore]
fn retentions_update_roundtrip() {
    if harness::require_write_creds("retentions_update_roundtrip").is_none() {
        return;
    }
    let before = retentions();
    // Only editable tags may be named; `Default` is read-only (403).
    let elements: Vec<Value> = before
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["editable"] == json!(true))
        .map(|r| json!({ "id": r["id"], "name": r["name"] }))
        .collect();
    if elements.is_empty() {
        eprintln!("[e2e] skipping retentions_update_roundtrip: no editable retention tags");
        return;
    }
    let file = harness::JsonFile::new(
        "retentions-upd",
        &json!({ "retentionUpdateElements": elements }),
    );
    let v = harness::run_write_json(&[
        "retentions",
        "update",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert!(
        v.get("retentions").is_some(),
        "update should echo the retentions: {v}"
    );
    assert_eq!(
        retentions(),
        before,
        "writing the same names back must not change the tags"
    );
}

/// `retentions activate` switches retention tags on for the whole team. It
/// is a one-way change to how the shared archive is written, so the test
/// never triggers it; it only reports the current state.
#[test]
#[ignore]
fn retentions_activate_not_covered() {
    if harness::require_creds("retentions_activate_not_covered").is_none() {
        return;
    }
    let status = harness::run_ok_json(&["retentions", "status", "-o", "json"]);
    let enabled = status["enableTags"]
        .as_bool()
        .or_else(|| status["enabled"].as_bool());
    match enabled {
        Some(true) => eprintln!("[e2e] not covered: retentions activate — already active on the test team, nothing to activate"),
        _ => eprintln!("[e2e] not covered: retentions activate — irreversible team-wide change, left for a human to run once"),
    }
}
