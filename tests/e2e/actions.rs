//! E2E coverage for `cx webhooks actions` (automation actions).
//!
//! Actions created with an API key must be shared (`isPrivate: false`): a
//! private action needs a user. Names are capped at 50 characters, so the
//! name slug is short (`act`) and the rename suffix is two characters.

use std::sync::OnceLock;
use std::time::Duration;

use crate::harness;
use serde_json::{json, Value};

#[test]
#[ignore]
fn actions_list() {
    if harness::require_creds("actions_list").is_none() {
        return;
    }
    // The writable e2e team starts with no actions; lifecycle tests create
    // their own, so only the shape is asserted here.
    let v = harness::run_ok_json(&["webhooks", "actions", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name", "url"]);
}

#[test]
#[ignore]
fn actions_get() {
    if harness::require_creds("actions_get").is_none() {
        return;
    }
    let id = match discover_action_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping actions_get: no actions available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["webhooks", "actions", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn actions_get_nonexistent_fails() {
    if harness::require_creds("actions_get_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&[
        "webhooks",
        "actions",
        "get",
        "00000000-0000-0000-0000-000000000000",
        "-o",
        "json",
    ]);
}

fn discover_action_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("actions_discover")?;
            let stdout = harness::run_ok(&["webhooks", "actions", "list", "-o", "json"]);
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

/// Bare action body (create takes it flat; replace wraps it in `action`).
/// `isHidden` is left out: create rejects it as an unknown field, while
/// replace requires it, so the update paths add it explicitly.
fn action_body(name: &str) -> Value {
    json!({
        "name": name,
        "url": format!("https://example.com/cx-e2e/{name}?q={{{{$d.text}}}}"),
        "isPrivate": false,
        "sourceType": "SOURCE_TYPE_LOG",
        "applicationNames": [],
        "subsystemNames": [],
        "urlFields": []
    })
}

/// Create an action; returns its id and an armed cleanup.
fn create_action(name: &str) -> (String, harness::Cleanup) {
    let file = harness::JsonFile::new("action", &action_body(name));
    let created = harness::run_write_json(&[
        "webhooks",
        "actions",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("action create returned no id: {created}"));
    assert_eq!(created["name"], name, "create response: {created}");
    let cleanup = harness::Cleanup::new(&["webhooks", "actions", "delete", &id]);
    (id, cleanup)
}

// ── Action lifecycle (writable e2e team) ────────────────────────────────────

#[test]
#[ignore]
fn actions_lifecycle() {
    if harness::require_write_creds("actions_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("act");

    // create
    let (id, mut cleanup) = create_action(&name);

    // get
    let got = harness::run_ok_json(&["webhooks", "actions", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name", "url"]);
    assert_eq!(got["action"]["name"], name, "get should return our name");
    assert_eq!(got["action"]["isPrivate"], false);

    // list contains it
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["webhooks", "actions", "list", "-o", "json"]);
        harness::find_by_name(&list, "name", &name)
            .is_some()
            .then_some(())
    });
    assert!(found.is_some(), "action list should contain {name}");

    // update: rename (replace takes the whole action incl. id)
    let renamed = format!("{name}-u");
    let mut upd_body = action_body(&renamed);
    upd_body["id"] = json!(id);
    upd_body["isHidden"] = json!(false);
    let upd = harness::JsonFile::new("action-upd", &upd_body);
    let updated = harness::run_write_json(&[
        "webhooks",
        "actions",
        "update",
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    assert_eq!(
        updated["action"]["name"], renamed,
        "update response: {updated}"
    );
    let got = harness::run_ok_json(&["webhooks", "actions", "get", &id, "-o", "json"]);
    assert_eq!(got["action"]["name"], renamed, "update should rename");

    // reorder: only our own action is named, so other actions keep their
    // relative order
    let order =
        harness::JsonFile::new("action-order", &json!({ "sharedActionsOrder": { &id: 0 } }));
    harness::run_write_ok(&[
        "webhooks",
        "actions",
        "reorder",
        "--from-file",
        order.path(),
        "-o",
        "json",
    ]);

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["webhooks", "actions", "get", &id]);
    harness::run_ok_nonempty(&["webhooks", "actions", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["webhooks", "actions", "list"]);

    // delete, then get must fail
    harness::run_write_ok(&["webhooks", "actions", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["webhooks", "actions", "get", &id, "-o", "json"]);
    harness::run_fail(&["webhooks", "actions", "delete", &id, "--yes"]);
}

#[test]
#[ignore]
fn actions_batch() {
    if harness::require_write_creds("actions_batch").is_none() {
        return;
    }
    let name_a = harness::unique_name("act");
    let name_b = harness::unique_name("act");

    // batch create two actions in one atomic request
    let batch = json!({ "requests": [
        { "create": action_body(&name_a) },
        { "create": action_body(&name_b) }
    ] });
    let file = harness::JsonFile::new("action-batch", &batch);
    let v = harness::run_write_json(&[
        "webhooks",
        "actions",
        "batch",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let responses = v["matchingResponses"]
        .as_array()
        .unwrap_or_else(|| panic!("batch should return matchingResponses: {v}"));
    assert_eq!(responses.len(), 2, "one response per request: {v}");
    let id_of = |i: usize| -> String {
        responses[i]["create"]["action"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("batch response {i} has no created id: {v}"))
            .to_string()
    };
    let (id_a, id_b) = (id_of(0), id_of(1));
    let mut cleanup_a = harness::Cleanup::new(&["webhooks", "actions", "delete", &id_a]);
    let mut cleanup_b = harness::Cleanup::new(&["webhooks", "actions", "delete", &id_b]);
    assert_eq!(responses[0]["create"]["action"]["name"], name_a);
    assert_eq!(responses[1]["create"]["action"]["name"], name_b);

    // both are visible
    let both = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["webhooks", "actions", "list", "-o", "json"]);
        (harness::find_by_name(&list, "name", &name_a).is_some()
            && harness::find_by_name(&list, "name", &name_b).is_some())
        .then_some(())
    });
    assert!(
        both.is_some(),
        "action list should contain {name_a} and {name_b}"
    );

    // batch delete one, replace the other, in one request
    let renamed_b = format!("{name_b}-u");
    let mut replaced_b = action_body(&renamed_b);
    replaced_b["id"] = json!(id_b);
    replaced_b["isHidden"] = json!(false);
    let batch = json!({ "requests": [
        { "delete": { "id": id_a } },
        { "replace": { "action": replaced_b } }
    ] });
    let file = harness::JsonFile::new("action-batch-2", &batch);
    let v = harness::run_write_json(&[
        "webhooks",
        "actions",
        "batch",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert_eq!(
        v["matchingResponses"].as_array().map(Vec::len),
        Some(2),
        "one response per request: {v}"
    );
    harness::run_fail(&["webhooks", "actions", "get", &id_a, "-o", "json"]);
    cleanup_a.disarm();
    let got = harness::run_ok_json(&["webhooks", "actions", "get", &id_b, "-o", "json"]);
    assert_eq!(
        got["action"]["name"], renamed_b,
        "batch replace should rename"
    );

    harness::run_write_ok(&["webhooks", "actions", "delete", &id_b]);
    cleanup_b.disarm();
    harness::run_fail(&["webhooks", "actions", "get", &id_b, "-o", "json"]);
}
