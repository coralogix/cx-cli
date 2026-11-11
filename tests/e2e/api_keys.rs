use std::sync::OnceLock;
use std::time::Duration;

use crate::harness;
use serde_json::{json, Value};

const NONEXISTENT_KEY_ID: &str = "00000000-0000-0000-0000-000000000000";

#[test]
#[ignore]
fn api_keys_list() {
    if harness::require_creds("api_keys_list").is_none() {
        return;
    }
    // The test team has seed keys (the suite's own key among them).
    let v = harness::run_ok_json(&["iam", "api-keys", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name", "active"]);
}

#[test]
#[ignore]
fn api_keys_send_data_keys() {
    if harness::require_creds("api_keys_send_data_keys").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["iam", "api-keys", "send-data-keys", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name", "presets"]);
}

#[test]
#[ignore]
fn api_keys_admin_list() {
    if harness::require_creds("api_keys_admin_list").is_none() {
        return;
    }
    // Team members' (user-owned) keys; may be empty.
    let v = harness::run_ok_json(&["iam", "api-keys", "admin", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["api_key", "permissions", "presets"]);
}

#[test]
#[ignore]
fn api_keys_get_nonexistent_fails() {
    if harness::require_creds("api_keys_get_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&["iam", "api-keys", "get", NONEXISTENT_KEY_ID, "-o", "json"]);
}

/// The team id, read from the seed team groups (every group carries it).
fn team_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["iam", "groups", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .find_map(|g| g.get("team_id").and_then(Value::as_i64))
                .map(|id| id.to_string())
        })
        .clone()
}

/// An active team member to own the key used by the admin tests. The admin
/// endpoints (`all/status`, `all/delete`) only accept user-owned keys, so the
/// test must create its key on behalf of a user; the key is named with
/// `unique_name` and removed again inside the test.
fn active_user_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&[
                "iam",
                "users",
                "search",
                "--status",
                "USER_STATUS_ACTIVE",
                "-o",
                "json",
            ]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .find_map(|u| u.get("user_id").and_then(Value::as_str))
                .map(str::to_string)
        })
        .clone()
}

fn key_body(name: &str, owner: Value) -> Value {
    json!({
        "name": name,
        "owner": owner,
        "keyPermissions": { "permissions": ["alerts:ReadConfig"], "presets": [] },
        "hashed": false
    })
}

/// `api-keys get` wraps the key in `keyInfo`.
fn key_info(v: &Value) -> Value {
    v.get("keyInfo").cloned().unwrap_or_else(|| v.clone())
}

fn get_key(id: &str) -> Value {
    key_info(&harness::run_ok_json(&[
        "iam", "api-keys", "get", id, "-o", "json",
    ]))
}

#[test]
#[ignore]
fn api_keys_lifecycle() {
    if harness::require_write_creds("api_keys_lifecycle").is_none() {
        return;
    }
    let Some(team) = team_id() else {
        eprintln!("[e2e] skipping api_keys_lifecycle: could not resolve the team id");
        return;
    };
    let name = harness::unique_name("key");
    let team: i64 = team.parse().expect("numeric team id");

    // create (team-owned key)
    let file = harness::JsonFile::new("key", &key_body(&name, json!({ "teamId": team })));
    let created = harness::run_write_json(&[
        "iam",
        "api-keys",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = created["key_id"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| panic!("api key create returned no key_id: {created}"));
    assert_eq!(created["name"], name);
    assert!(
        created["value"].as_str().is_some_and(|v| !v.is_empty()),
        "create should return the key value once"
    );
    let mut cleanup = harness::Cleanup::new(&["iam", "api-keys", "delete", &id]);

    // get
    let got = get_key(&id);
    assert_eq!(got["id"], id);
    assert_eq!(got["name"], name);
    assert_eq!(got["active"], true);

    // list contains it
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["iam", "api-keys", "list", "-o", "json"]);
        harness::find_by_name(&list, "name", &name).map(|_| ())
    });
    assert!(found.is_some(), "api-keys list should contain {name}");

    // update: rename
    let renamed = format!("{name}-renamed");
    let upd = harness::JsonFile::new("key-upd", &json!({ "newName": renamed }));
    harness::run_write_ok(&[
        "iam",
        "api-keys",
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    assert_eq!(
        get_key(&id)["name"],
        renamed,
        "update should rename the key"
    );

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["iam", "api-keys", "get", &id]);
    harness::run_ok_nonempty(&["iam", "api-keys", "get", &id, "-o", "toon"]);

    // delete, then get must fail
    harness::run_write_ok(&["iam", "api-keys", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["iam", "api-keys", "get", &id, "-o", "json"]);
    harness::run_fail(&["iam", "api-keys", "delete", NONEXISTENT_KEY_ID, "--yes"]);
}

#[test]
#[ignore]
fn api_keys_admin_lifecycle() {
    if harness::require_write_creds("api_keys_admin_lifecycle").is_none() {
        return;
    }
    let Some(user) = active_user_id() else {
        eprintln!("[e2e] skipping api_keys_admin_lifecycle: no active user on the test team");
        return;
    };
    let name = harness::unique_name("ukey");

    // create a user-owned key: the admin endpoints only act on those
    let file = harness::JsonFile::new("ukey", &key_body(&name, json!({ "userId": user })));
    let created = harness::run_write_json(&[
        "iam",
        "api-keys",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = created["key_id"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| panic!("api key create returned no key_id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["iam", "api-keys", "delete", &id]);

    // admin list contains it
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["iam", "api-keys", "admin", "list", "-o", "json"]);
        list.as_array()?
            .iter()
            .find(|k| k.pointer("/api_key/id").and_then(Value::as_str) == Some(&id))
            .map(|_| ())
    });
    assert!(found.is_some(), "admin list should contain key {id}");

    // admin set-status: deactivate (no --active flag), then activate
    harness::run_write_ok(&["iam", "api-keys", "admin", "set-status", "--ids", &id]);
    assert_eq!(get_key(&id)["active"], false, "key should be inactive");
    harness::run_write_ok(&[
        "iam",
        "api-keys",
        "admin",
        "set-status",
        "--ids",
        &id,
        "--active",
    ]);
    assert_eq!(get_key(&id)["active"], true, "key should be active again");

    // admin delete, then get must fail
    harness::run_write_ok(&["iam", "api-keys", "admin", "delete", "--ids", &id]);
    cleanup.disarm();
    harness::run_fail(&["iam", "api-keys", "get", &id, "-o", "json"]);
    harness::run_fail(&[
        "iam",
        "api-keys",
        "admin",
        "delete",
        "--ids",
        NONEXISTENT_KEY_ID,
        "--yes",
    ]);
}
