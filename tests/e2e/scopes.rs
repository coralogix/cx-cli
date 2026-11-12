use std::time::Duration;

use crate::harness;
use serde_json::json;

const NONEXISTENT_SCOPE_ID: &str = "00000000-0000-0000-0000-000000000000";

#[test]
#[ignore]
fn scopes_list() {
    if harness::require_creds("scopes_list").is_none() {
        return;
    }
    // The writable e2e team starts with no scopes; the lifecycle test creates
    // its own, so only the shape is asserted here.
    let v = harness::run_ok_json(&["iam", "scopes", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn scopes_get_nonexistent_fails() {
    if harness::require_creds("scopes_get_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&["iam", "scopes", "get", NONEXISTENT_SCOPE_ID, "-o", "json"]);
}

#[test]
#[ignore]
fn scopes_lifecycle() {
    if harness::require_write_creds("scopes_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("scope");

    // create
    let file = harness::JsonFile::new(
        "scope",
        &json!({
            "displayName": name,
            "description": "cx e2e scope",
            "defaultExpression": "<v1>true",
            "filters": [{ "entityType": "ENTITY_TYPE_LOGS", "expression": "<v1>true" }]
        }),
    );
    let created = harness::run_write_json(&[
        "iam",
        "scopes",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("scope create returned no id: {created}"));
    assert_eq!(created["name"], name);
    let mut cleanup = harness::Cleanup::new(&["iam", "scopes", "delete", &id]);

    // get (wrapped in `scope`)
    let got = harness::run_ok_json(&["iam", "scopes", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "displayName", "defaultExpression"]);
    assert_eq!(got["scope"]["id"], id);
    assert_eq!(got["scope"]["displayName"], name);
    assert_eq!(got["scope"]["filters"].as_array().map(Vec::len), Some(1));

    // list contains it
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["iam", "scopes", "list", "-o", "json"]);
        harness::find_by_name(&list, "name", &name).map(|_| ())
    });
    assert!(found.is_some(), "scopes list should contain {name}");

    // update (replace): rename and drop the filters
    let renamed = format!("{name}-renamed");
    let upd = harness::JsonFile::new(
        "scope-upd",
        &json!({
            "id": id,
            "displayName": renamed,
            "description": "cx e2e scope updated",
            "defaultExpression": "<v1>true",
            "filters": []
        }),
    );
    harness::run_write_ok(&[
        "iam",
        "scopes",
        "update",
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let got = harness::run_ok_json(&["iam", "scopes", "get", &id, "-o", "json"]);
    assert_eq!(
        got["scope"]["displayName"], renamed,
        "scope update should rename"
    );
    assert_eq!(got["scope"]["filters"], json!([]));

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["iam", "scopes", "get", &id]);
    harness::run_ok_nonempty(&["iam", "scopes", "get", &id, "-o", "toon"]);

    // delete, then get must fail. (The API answers 200 for a delete of an
    // unknown scope id, so there is no negative delete path to assert.)
    harness::run_write_ok(&["iam", "scopes", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["iam", "scopes", "get", &id, "-o", "json"]);
}
