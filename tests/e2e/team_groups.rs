use std::sync::OnceLock;
use std::time::Duration;

use crate::harness;
use serde_json::json;

#[test]
#[ignore]
fn team_groups_list() {
    if harness::require_creds("team_groups_list").is_none() {
        return;
    }
    // The test team has seed groups.
    let v = harness::run_ok_json(&["iam", "groups", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["group_id", "name", "team_id"]);
}

#[test]
#[ignore]
fn team_groups_get() {
    if harness::require_creds("team_groups_get").is_none() {
        return;
    }
    let id = match discover_team_group_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping team_groups_get: no team groups available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["iam", "groups", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["group_id", "name"]);
    assert_eq!(v["group_id"], id);
}

#[test]
#[ignore]
fn team_groups_get_by_name_nonexistent_fails() {
    if harness::require_creds("team_groups_get_by_name_nonexistent_fails").is_none() {
        return;
    }
    let name = harness::unique_name("nogroup");
    harness::run_fail(&["iam", "groups", "get-by-name", &name, "-o", "json"]);
}

#[test]
#[ignore]
fn team_groups_lifecycle() {
    if harness::require_write_creds("team_groups_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("group");

    // create (role by built-in name; no members)
    let file = harness::JsonFile::new(
        "group",
        &json!({
            "name": name,
            "description": "cx e2e group",
            "roleName": "Read-Only User",
            "userIds": []
        }),
    );
    let created = harness::run_write_json(&[
        "iam",
        "groups",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = created["group_id"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| panic!("group create returned no group_id: {created}"));
    assert_eq!(created["name"], name);
    let mut cleanup = harness::Cleanup::new(&["iam", "groups", "delete", &id]);

    // get by id and by name
    let got = harness::run_ok_json(&["iam", "groups", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["group_id", "name"]);
    assert_eq!(got["name"], name);
    let by_name = harness::run_ok_json(&["iam", "groups", "get-by-name", &name, "-o", "json"]);
    assert_eq!(by_name["group_id"], id);

    // users of the (empty) group
    let users = harness::run_ok_json(&["iam", "groups", "users", &id, "-o", "json"]);
    assert_eq!(users, json!([]), "new group should have no users");

    // list contains it
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["iam", "groups", "list", "-o", "json"]);
        harness::find_by_name(&list, "name", &name).map(|_| ())
    });
    assert!(found.is_some(), "groups list should contain {name}");

    // update: rename + description
    let renamed = format!("{name}-renamed");
    let upd = harness::JsonFile::new(
        "group-upd",
        &json!({ "name": renamed, "description": "cx e2e group updated" }),
    );
    let updated = harness::run_write_json(&[
        "iam",
        "groups",
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    assert_eq!(updated["name"], renamed);
    let got = harness::run_ok_json(&["iam", "groups", "get", &id, "-o", "json"]);
    assert_eq!(got["name"], renamed, "group update should rename");
    assert_eq!(got["description"], "cx e2e group updated");

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["iam", "groups", "get", &id]);
    harness::run_ok_nonempty(&["iam", "groups", "get", &id, "-o", "toon"]);

    // delete, then get must fail
    harness::run_write_ok(&["iam", "groups", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["iam", "groups", "get", &id, "-o", "json"]);
    harness::run_fail(&["iam", "groups", "delete", &id, "--yes"]);
}

fn discover_team_group_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("team_groups_discover")?;
            let stdout = harness::run_ok(&["iam", "groups", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .find(|item| !harness::is_e2e_item(item))
                .and_then(|item| item.get("group_id"))
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
        })
        .clone()
}
