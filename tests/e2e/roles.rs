use std::sync::OnceLock;
use std::time::Duration;

use crate::harness;
use serde_json::json;

const NONEXISTENT_ROLE_ID: &str = "999999999";

#[test]
#[ignore]
fn roles_list() {
    if harness::require_creds("roles_list").is_none() {
        return;
    }
    // The writable e2e team starts with no custom roles; the lifecycle test
    // creates its own, so only the shape is asserted here.
    let v = harness::run_ok_json(&["iam", "roles", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["role_id", "name"]);
}

#[test]
#[ignore]
fn roles_system() {
    if harness::require_creds("roles_system").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["iam", "roles", "system", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["role_id", "name"]);
}

#[test]
#[ignore]
fn roles_get_nonexistent_fails() {
    if harness::require_creds("roles_get_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&["iam", "roles", "get", NONEXISTENT_ROLE_ID, "-o", "json"]);
}

/// Id of a built-in system role to use as the parent of created roles.
fn parent_role_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["iam", "roles", "system", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            let roles = v.as_array()?;
            roles
                .iter()
                .find(|r| r["name"].as_str() == Some("Read-Only User"))
                .or_else(|| roles.first())
                .and_then(|r| r.get("role_id"))
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
        })
        .clone()
}

#[test]
#[ignore]
fn roles_lifecycle() {
    if harness::require_write_creds("roles_lifecycle").is_none() {
        return;
    }
    let Some(parent) = parent_role_id() else {
        eprintln!("[e2e] skipping roles_lifecycle: no system roles available");
        return;
    };
    let parent: i64 = parent.parse().expect("numeric system role id");
    let name = harness::unique_name("role");

    // create
    let file = harness::JsonFile::new(
        "role",
        &json!({
            "name": name,
            "description": "cx e2e role",
            "parentRoleId": parent,
            "permissions": []
        }),
    );
    let created = harness::run_write_json(&[
        "iam",
        "roles",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("role create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["iam", "roles", "delete", &id]);

    // get
    let got = harness::run_ok_json(&["iam", "roles", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["role_id", "name"]);
    assert_eq!(got["role_id"], id);
    assert_eq!(got["name"], name);
    assert_eq!(got["parent_role_id"], parent.to_string());

    // list contains it
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["iam", "roles", "list", "-o", "json"]);
        harness::find_by_name(&list, "name", &name).map(|_| ())
    });
    assert!(found.is_some(), "roles list should contain {name}");

    // update: rename + new description
    let renamed = format!("{name}-renamed");
    let upd = harness::JsonFile::new(
        "role-upd",
        &json!({ "newName": renamed, "newDescription": "cx e2e role updated" }),
    );
    harness::run_write_ok(&[
        "iam",
        "roles",
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let got = harness::run_ok_json(&["iam", "roles", "get", &id, "-o", "json"]);
    assert_eq!(got["name"], renamed, "role update should rename");
    assert_eq!(got["description"], "cx e2e role updated");

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["iam", "roles", "get", &id]);
    harness::run_ok_nonempty(&["iam", "roles", "get", &id, "-o", "toon"]);

    // delete, then get must fail
    harness::run_write_ok(&["iam", "roles", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["iam", "roles", "get", &id, "-o", "json"]);
    harness::run_fail(&["iam", "roles", "delete", NONEXISTENT_ROLE_ID, "--yes"]);
}
