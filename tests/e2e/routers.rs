//! E2E coverage for `cx notifications routers`.
//!
//! A router needs at least one routing label and a target connector. The
//! lifecycle test creates its own GENERIC_HTTPS connector (see
//! `connectors.rs`), routes to it with the matching system preset, and
//! deletes the router before the connector: the API refuses to delete a
//! connector that a router still references.

use std::sync::OnceLock;
use std::time::Duration;

use crate::connectors::{create_generic_https_connector, GENERIC_HTTPS_PRESET};
use crate::harness;
use serde_json::{json, Value};

#[test]
#[ignore]
fn routers_list() {
    if harness::require_creds("routers_list").is_none() {
        return;
    }
    // The writable e2e team starts with no routers; the lifecycle test
    // creates its own, so only the shape is asserted here.
    let v = harness::run_ok_json(&["notifications", "routers", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn routers_get() {
    if harness::require_creds("routers_get").is_none() {
        return;
    }
    let id = match discover_router_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping routers_get: no routers available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["notifications", "routers", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn routers_get_nonexistent_fails() {
    if harness::require_creds("routers_get_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&[
        "notifications",
        "routers",
        "get",
        "00000000-0000-0000-0000-000000000000",
        "-o",
        "json",
    ]);
}

fn discover_router_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("routers_discover")?;
            let stdout = harness::run_ok(&["notifications", "routers", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .find(|item| !harness::is_e2e_item(item))
                .and_then(|item| item.get("id"))
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string().trim_matches('"').to_string(),
                })
        })
        .clone()
}

/// Router body: one P1 rule targeting `connector_id`, labelled with our
/// unique name so no other run's router can match the same labels.
fn router_body(name: &str, connector_id: &str) -> Value {
    json!({
        "router": {
            "name": name,
            "description": "cx e2e",
            "routingLabels": { "environment": name, "service": "cx-e2e" },
            "rules": [{
                "name": "p1",
                "condition": "alertDef.priority == 'P1'",
                "entityType": "ALERTS",
                "targets": [{ "connectorId": connector_id, "presetId": GENERIC_HTTPS_PRESET }]
            }],
            "fallbackTargets": []
        }
    })
}

// ── Router lifecycle (writable e2e team) ────────────────────────────────────

#[test]
#[ignore]
fn routers_lifecycle() {
    if harness::require_write_creds("routers_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("router");

    // the connector the router routes to (dropped last, see module docs)
    let (connector_id, mut connector_cleanup) = create_generic_https_connector(&name);

    // create
    let body = router_body(&name, &connector_id);
    let file = harness::JsonFile::new("router", &body);
    let created = harness::run_write_json(&[
        "notifications",
        "routers",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("router create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["notifications", "routers", "delete", &id]);

    // get
    let got = harness::run_ok_json(&["notifications", "routers", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name", "rules"]);
    assert_eq!(got["router"]["name"], name);
    assert_eq!(got["router"]["routingLabels"]["environment"], name);
    assert_eq!(
        got["router"]["rules"][0]["targets"][0]["connectorId"], connector_id,
        "rule should target our connector: {got}"
    );

    // list contains it
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["notifications", "routers", "list", "-o", "json"]);
        harness::find_by_name(&list, "name", &name)
            .is_some()
            .then_some(())
    });
    assert!(found.is_some(), "router list should contain {name}");

    // update: rename and add a label (PUT replaces the whole router)
    let renamed = format!("{name}-renamed");
    let mut upd_body = body.clone();
    upd_body["router"]["id"] = json!(id);
    upd_body["router"]["name"] = json!(renamed);
    upd_body["router"]["routingLabels"]["team"] = json!("cx-e2e");
    let upd = harness::JsonFile::new("router-upd", &upd_body);
    let updated = harness::run_write_json(&[
        "notifications",
        "routers",
        "update",
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    assert_eq!(
        updated["router"]["name"], renamed,
        "update response: {updated}"
    );
    let got = harness::run_ok_json(&["notifications", "routers", "get", &id, "-o", "json"]);
    assert_eq!(got["router"]["name"], renamed, "update should rename");
    assert_eq!(got["router"]["routingLabels"]["team"], "cx-e2e");

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["notifications", "routers", "get", &id]);
    harness::run_ok_nonempty(&["notifications", "routers", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["notifications", "routers", "list"]);

    // the connector is in use: its delete must be refused while the router
    // exists
    harness::run_fail(&[
        "notifications",
        "connectors",
        "delete",
        &connector_id,
        "--yes",
    ]);

    // delete router, then get must fail (a second delete is idempotent,
    // exit 0)
    harness::run_write_ok(&["notifications", "routers", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["notifications", "routers", "get", &id, "-o", "json"]);

    // now the connector can go
    harness::run_write_ok(&["notifications", "connectors", "delete", &connector_id]);
    connector_cleanup.disarm();
}

#[test]
#[ignore]
fn routers_validate_matcher() {
    if harness::require_creds("routers_validate_matcher").is_none() {
        return;
    }
    // Read-only: the API checks the label keys and answers success/failure.
    let good = json!({ "entityLabelMatcher": { "routing.environment": "cx-e2e" } });
    let file = harness::JsonFile::new("matcher", &good);
    let v = harness::run_ok_json(&[
        "notifications",
        "routers",
        "validate-matcher",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert!(
        v.pointer("/result/success").is_some(),
        "routing.* keys should validate: {v}"
    );
    harness::run_ok_nonempty(&[
        "notifications",
        "routers",
        "validate-matcher",
        "--from-file",
        file.path(),
    ]);

    // A key outside routing.{team,service,environment} is reported as a
    // failure verdict, still with exit 0.
    let bad = json!({ "not-a-routing-key": "x" });
    let file = harness::JsonFile::new("matcher-bad", &bad);
    let v = harness::run_ok_json(&[
        "notifications",
        "routers",
        "validate-matcher",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert!(
        v.pointer("/result/failure/message").is_some(),
        "unknown label key should be a failure verdict: {v}"
    );
}
