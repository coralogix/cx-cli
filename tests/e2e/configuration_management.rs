//! `cx configuration-groups` and `cx configuration-overlays` E2E coverage.
//!
//! Write lifecycles keep both resources inactive. The group family has no
//! matching agent selector and is inactive; the overlay has no targets and is
//! inactive. These tests cover management behavior without applying a config
//! to a collector.

use std::collections::HashSet;

use crate::harness;
use serde_json::json;

const RAW_CONFIGURATION: &str = "receivers:\n  otlp:\n    protocols:\n      grpc: {}\n      http: {}\nexporters:\n  debug: {}\nservice:\n  pipelines:\n    traces:\n      receivers: [otlp]\n      exporters: [debug]\n";
const OVERLAY_CONFIGURATION: &str = "service:\n  telemetry:\n    logs:\n      level: debug\n";

/// Follow the per-profile continuation token until the resource appears or
/// the API has no more pages. The test team may contain enough archived
/// resources that an E2E resource is not in the first page.
fn list_contains_id(command: &str, collection: &str, id: &str, include_archived: bool) -> bool {
    let mut page_token = None;
    let mut seen_tokens = HashSet::new();
    loop {
        let mut args = vec![command, "list"];
        if include_archived {
            args.push("--include-archived");
        }
        args.extend(["--page-size", "256"]);
        if let Some(token) = page_token.as_deref() {
            args.extend(["--page-token", token]);
        }
        args.extend(["-o", "json"]);

        let response = harness::run_ok_json(&args);
        if response
            .get(collection)
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.get("id").and_then(serde_json::Value::as_str) == Some(id))
            })
        {
            return true;
        }

        let next_token = response
            .get("next_page_tokens")
            .and_then(serde_json::Value::as_object)
            .and_then(|tokens| tokens.values().find_map(serde_json::Value::as_str))
            .map(str::to_string);
        let Some(next_token) = next_token else {
            return false;
        };
        if !seen_tokens.insert(next_token.clone()) {
            return false;
        }
        page_token = Some(next_token);
    }
}

#[test]
#[ignore]
fn configuration_groups_list() {
    if harness::require_creds("configuration_groups_list").is_none() {
        return;
    }

    let groups = harness::run_ok_json(&["configuration-groups", "list", "-o", "json"]);
    harness::assert_object_with_keys(&groups, &["groups"]);
    harness::assert_array(&groups["groups"]);
    harness::run_ok_nonempty(&["configuration-groups", "list"]);
    harness::run_ok_nonempty(&["configuration-groups", "list", "-o", "toon"]);

    let page = harness::run_ok_json(&[
        "configuration-groups",
        "list",
        "--page-size",
        "1",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&page, &["groups"]);
    assert!(page["groups"].as_array().unwrap().len() <= 1);
}

#[test]
#[ignore]
fn configuration_groups_lifecycle() {
    if harness::require_write_creds("configuration_groups_lifecycle").is_none() {
        return;
    }

    let name = harness::unique_name("cfg-group");
    let body = json!({
        "name": name,
        "description": "cx e2e inactive configuration group",
        "family": {
            "active": false,
            "raw": {
                "remoteConfigurations": [{
                    "name": "manual-test",
                    "rawConfiguration": RAW_CONFIGURATION
                }]
            }
        }
    });
    let file = harness::JsonFile::new("cfg-group", &body);
    let created = harness::run_write_json(&[
        "configuration-groups",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("configuration group create returned no id: {created}"));
    assert_eq!(created["name"], name);
    let mut cleanup = harness::Cleanup::new(&["configuration-groups", "archive", &id]);

    let got = harness::run_ok_json(&["configuration-groups", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name", "family"]);
    assert_eq!(got["id"], id);
    assert_eq!(got["name"], name);
    assert_eq!(got.pointer("/family/active"), Some(&json!(false)));
    let stored_configuration = got
        .pointer("/family/raw/remoteConfigurations/0/rawConfiguration")
        .and_then(serde_json::Value::as_str)
        .expect("group should return its raw Collector configuration");
    assert!(stored_configuration.contains("otlp"));
    assert!(stored_configuration.contains("debug"));

    assert!(
        list_contains_id("configuration-groups", "groups", &id, false),
        "configuration group list should contain {id}"
    );
    harness::run_ok_nonempty(&["configuration-groups", "get", &id]);
    harness::run_ok_nonempty(&["configuration-groups", "get", &id, "-o", "toon"]);

    let update = harness::JsonFile::new(
        "cfg-group-update",
        &json!({"description": "cx e2e group updated"}),
    );
    let updated = harness::run_write_json(&[
        "configuration-groups",
        "update",
        &id,
        "--update-mask",
        "description",
        "--from-file",
        update.path(),
        "-o",
        "json",
    ]);
    assert_eq!(updated["description"], "cx e2e group updated");
    let got = harness::run_ok_json(&["configuration-groups", "get", &id, "-o", "json"]);
    assert_eq!(got["description"], "cx e2e group updated");
    assert_eq!(got.pointer("/family/active"), Some(&json!(false)));

    harness::run_write_ok(&["configuration-groups", "archive", &id]);
    cleanup.disarm();
    harness::run_fail(&["configuration-groups", "get", &id, "-o", "json"]);
    let archived = harness::run_ok_json(&[
        "configuration-groups",
        "get",
        &id,
        "--include-archived",
        "-o",
        "json",
    ]);
    assert_eq!(archived["archived"], json!(true));
    assert!(
        list_contains_id("configuration-groups", "groups", &id, true),
        "archived group list should contain {id}"
    );
}

#[test]
#[ignore]
fn configuration_overlays_list() {
    if harness::require_creds("configuration_overlays_list").is_none() {
        return;
    }

    let overlays = harness::run_ok_json(&["configuration-overlays", "list", "-o", "json"]);
    harness::assert_object_with_keys(&overlays, &["overlays"]);
    harness::assert_array(&overlays["overlays"]);
    harness::run_ok_nonempty(&["configuration-overlays", "list"]);
    harness::run_ok_nonempty(&["configuration-overlays", "list", "-o", "toon"]);

    let page = harness::run_ok_json(&[
        "configuration-overlays",
        "list",
        "--page-size",
        "1",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&page, &["overlays"]);
    assert!(page["overlays"].as_array().unwrap().len() <= 1);
}

#[test]
#[ignore]
fn configuration_overlays_lifecycle() {
    if harness::require_write_creds("configuration_overlays_lifecycle").is_none() {
        return;
    }

    let name = harness::unique_name("cfg-overlay");
    let body = json!({
        "name": name,
        "description": "cx e2e inactive configuration overlay",
        "active": false,
        "custom": {
            "configuration": OVERLAY_CONFIGURATION,
            "targets": []
        }
    });
    let file = harness::JsonFile::new("cfg-overlay", &body);
    let created = harness::run_write_json(&[
        "configuration-overlays",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("configuration overlay create returned no id: {created}"));
    assert_eq!(created["name"], name);
    let mut cleanup = harness::Cleanup::new(&["configuration-overlays", "archive", &id]);

    let got = harness::run_ok_json(&["configuration-overlays", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name", "custom"]);
    assert_eq!(got["id"], id);
    assert_eq!(got["name"], name);
    assert_eq!(got["active"], json!(false));
    let stored_configuration = got
        .pointer("/custom/configuration")
        .and_then(serde_json::Value::as_str)
        .expect("overlay should return its custom configuration");
    assert!(stored_configuration.contains("debug"));
    assert!(got
        .pointer("/custom/targets")
        .and_then(serde_json::Value::as_array)
        .map(Vec::is_empty)
        .unwrap_or(true));

    assert!(
        list_contains_id("configuration-overlays", "overlays", &id, false),
        "configuration overlay list should contain {id}"
    );
    harness::run_ok_nonempty(&["configuration-overlays", "get", &id]);
    harness::run_ok_nonempty(&["configuration-overlays", "get", &id, "-o", "toon"]);

    let update = harness::JsonFile::new(
        "cfg-overlay-update",
        &json!({"description": "cx e2e overlay updated"}),
    );
    let updated = harness::run_write_json(&[
        "configuration-overlays",
        "update",
        &id,
        "--update-mask",
        "description",
        "--from-file",
        update.path(),
        "-o",
        "json",
    ]);
    assert_eq!(updated["description"], "cx e2e overlay updated");
    let got = harness::run_ok_json(&["configuration-overlays", "get", &id, "-o", "json"]);
    assert_eq!(got["description"], "cx e2e overlay updated");
    assert_eq!(got["active"], json!(false));
    assert!(got
        .pointer("/custom/targets")
        .and_then(serde_json::Value::as_array)
        .map(Vec::is_empty)
        .unwrap_or(true));

    harness::run_write_ok(&["configuration-overlays", "archive", &id]);
    cleanup.disarm();
    harness::run_fail(&["configuration-overlays", "get", &id, "-o", "json"]);
    let archived = harness::run_ok_json(&[
        "configuration-overlays",
        "get",
        &id,
        "--include-archived",
        "-o",
        "json",
    ]);
    assert_eq!(archived["archived"], json!(true));
    assert!(
        list_contains_id("configuration-overlays", "overlays", &id, true),
        "archived overlay list should contain {id}"
    );
}
