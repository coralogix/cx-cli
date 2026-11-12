use std::time::Duration;

use crate::custom_enrichments::create_table;
use crate::harness;
use serde_json::{json, Value};

#[test]
#[ignore]
fn enrichments_list() {
    if harness::require_creds("enrichments_list").is_none() {
        return;
    }
    // `list` returns `{"enrichments": [...]}`; the team may have none.
    let v = harness::run_ok_json(&["enrichments", "list", "-o", "json"]);
    harness::assert_object_with_keys(&v, &["enrichments"]);
    harness::assert_array_of_objects_with_keys(&v["enrichments"], &["id", "fieldName"]);
}

#[test]
#[ignore]
fn enrichments_limit() {
    if harness::require_creds("enrichments_limit").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["enrichments", "limit", "-o", "json"]);
    harness::assert_object_with_keys(&v, &["limit", "used"]);
}

#[test]
#[ignore]
fn enrichments_settings() {
    if harness::require_creds("enrichments_settings").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["enrichments", "settings", "-o", "json"]);
    harness::assert_get_response(&v, &["enrichmentAmountLimit", "rowLimit"]);
}

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

fn rules_of(list_response: &Value) -> &[Value] {
    list_response
        .get("enrichments")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn find_rule<'a>(list_response: &'a Value, field_name: &str) -> Option<&'a Value> {
    rules_of(list_response)
        .iter()
        .find(|r| r.get("fieldName").and_then(Value::as_str) == Some(field_name))
}

fn rule_id(rule: &Value) -> String {
    match &rule["id"] {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A rule of `customEnrichment` type on a field that no live log has
/// (our unique name), backed by a table this test created.
fn rule_body(field_name: &str, table_id: u64) -> Value {
    json!({
        "requestEnrichments": [
            { "fieldName": field_name, "enrichmentType": { "customEnrichment": { "id": table_id } } }
        ]
    })
}

#[test]
#[ignore]
fn enrichments_rules_lifecycle() {
    if harness::require_write_creds("enrichments_rules_lifecycle").is_none() {
        return;
    }
    let table_name = harness::unique_name("ce");
    let field_name = harness::unique_name("field");

    // a custom table to back the rule
    let table_id = create_table(&table_name);
    let _table_cleanup = harness::Cleanup::new(&["enrichments", "custom", "delete", &table_id]);
    let table_num: u64 = table_id.parse().expect("custom enrichment ids are numeric");

    // add
    let add = harness::JsonFile::new("enr-add", &rule_body(&field_name, table_num));
    let added = harness::run_write_json(&[
        "enrichments",
        "add",
        "--from-file",
        add.path(),
        "-o",
        "json",
    ]);
    let ours = find_rule(&added, &field_name)
        .unwrap_or_else(|| panic!("add response should contain our rule: {added}"));
    assert_eq!(
        ours.pointer("/enrichmentType/customEnrichment/id"),
        Some(&json!(table_num)),
        "rule should point at our table: {added}"
    );

    // Cleanup removes by id. Ids can change if another run overwrites the
    // set, so the explicit remove below re-reads the id from `list` first.
    let rm_file = harness::JsonFile::new("enr-rm", &json!({ "enrichmentIds": [rule_id(ours)] }));
    let mut rule_cleanup =
        harness::Cleanup::new(&["enrichments", "remove", "--from-file", rm_file.path()]);

    // list contains it
    let list = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["enrichments", "list", "-o", "json"]);
        let found = find_rule(&list, &field_name).is_some();
        found.then_some(list)
    })
    .unwrap_or_else(|| panic!("enrichment list should contain field {field_name}"));
    let current_id = rule_id(find_rule(&list, &field_name).unwrap());

    // text + toon renderers
    harness::run_ok_nonempty(&["enrichments", "list"]);
    harness::run_ok_nonempty(&["enrichments", "list", "-o", "toon"]);

    // remove, then list must not contain it
    let rm = harness::JsonFile::new("enr-rm2", &json!({ "enrichmentIds": [current_id] }));
    let removed = harness::run_write_json(&[
        "enrichments",
        "remove",
        "--from-file",
        rm.path(),
        "-o",
        "json",
    ]);
    rule_cleanup.disarm();
    harness::assert_object_with_keys(&removed, &["remainingEnrichments"]);
    let list = harness::run_ok_json(&["enrichments", "list", "-o", "json"]);
    assert!(
        find_rule(&list, &field_name).is_none(),
        "enrichment list should no longer contain {field_name}: {list}"
    );
}

/// `overwrite` (`PUT /enrichment-rules/v1/all`) is not exercised here.
///
/// It replaces every enrichment rule of the team, and, as verified against
/// the test team on 2026-11-11, it also deletes every custom enrichment
/// table that the new set does not reference. An "idempotent" round-trip
/// (list, overwrite with the same set, list again) is therefore not safe
/// while other tests run: any table created by a parallel test between the
/// `list` and the `overwrite` is destroyed. The HTTP path and body shape
/// are covered by the wiremock test `overwrite_puts_to_all` in
/// `tests/enrichments/main.rs`; the CLI's body validation is unit-tested
/// in `src/commands/enrichments/mod.rs`.
#[test]
#[ignore]
fn enrichments_overwrite_not_covered() {
    if harness::require_write_creds("enrichments_overwrite_not_covered").is_none() {
        return;
    }
    eprintln!(
        "[e2e] not covered: enrichments overwrite — the atomic overwrite endpoint also \
         deletes every custom enrichment table the new set does not reference, so a \
         round-trip would destroy tables created concurrently by other tests"
    );
}

#[test]
#[ignore]
fn enrichments_remove_rejects_bad_body() {
    if harness::require_write_creds("enrichments_remove_rejects_bad_body").is_none() {
        return;
    }
    // No ids: the CLI must refuse before calling the API.
    let file = harness::JsonFile::new("enr-rm-bad", &json!({ "requestEnrichments": [] }));
    let err = harness::run_fail(&["enrichments", "remove", "--from-file", file.path(), "--yes"]);
    assert!(
        err.contains("enrichmentIds"),
        "stderr should name the key: {err}"
    );
}
