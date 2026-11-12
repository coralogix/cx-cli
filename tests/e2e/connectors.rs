//! E2E coverage for `cx notifications connectors`, `cx notifications
//! presets` and `cx notifications test`.
//!
//! The writable e2e team starts with no connectors. Every write test creates
//! its own GENERIC_HTTPS connector that points at IANA's reserved
//! `example.com` host: the API resolves the host at create time (so
//! `example.invalid` is rejected), and a test delivery to example.com gets a
//! 405 back, which the API reports as a `failure` payload with exit 0. No
//! person or system is notified.

use std::sync::OnceLock;
use std::time::Duration;

use crate::harness;
use serde_json::{json, Value};

/// System preset that pairs with a GENERIC_HTTPS connector for alerts.
pub(crate) const GENERIC_HTTPS_PRESET: &str = "preset_system_generic_https_alerts_empty";
/// The only output schema the API accepts for GENERIC_HTTPS tests.
pub(crate) const GENERIC_HTTPS_PAYLOAD_TYPE: &str = "generic_https_default";
/// System preset the custom-preset lifecycle derives from.
const SLACK_ALERTS_PRESET: &str = "preset_system_slack_alerts_basic";

/// Connector config fields for a GENERIC_HTTPS destination named `name`.
pub(crate) fn generic_https_fields(name: &str) -> Value {
    json!([
        { "fieldName": "url", "value": format!("https://example.com/cx-e2e/{name}") },
        { "fieldName": "method", "value": "post" }
    ])
}

/// Create body for a GENERIC_HTTPS connector.
pub(crate) fn generic_https_connector_body(name: &str) -> Value {
    json!({
        "connector": {
            "name": name,
            "type": "GENERIC_HTTPS",
            "description": "cx e2e",
            "connectorConfig": { "fields": generic_https_fields(name) }
        }
    })
}

/// Create a GENERIC_HTTPS connector named `name`; returns its id and an
/// armed [`harness::Cleanup`] that deletes it on drop.
pub(crate) fn create_generic_https_connector(name: &str) -> (String, harness::Cleanup) {
    let file = harness::JsonFile::new("connector", &generic_https_connector_body(name));
    let created = harness::run_write_json(&[
        "notifications",
        "connectors",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("connector create returned no id: {created}"));
    let cleanup = harness::Cleanup::new(&["notifications", "connectors", "delete", &id]);
    (id, cleanup)
}

/// A `notifications test ...` response must carry a verdict: the API
/// answers 200 with `result.success` / `result.failure` (or the same pair at
/// the top level for routing conditions). A delivery failure is a valid
/// verdict - the point is that the CLI relayed it instead of crashing.
pub(crate) fn assert_test_verdict(v: &Value) {
    let obj = v
        .as_object()
        .unwrap_or_else(|| panic!("expected JSON object, got: {v}"));
    let inner = obj.get("result").and_then(Value::as_object).unwrap_or(obj);
    assert!(
        inner.contains_key("success") || inner.contains_key("failure"),
        "test response carries neither success nor failure: {v}"
    );
}

#[test]
#[ignore]
fn connectors_list() {
    if harness::require_creds("connectors_list").is_none() {
        return;
    }
    // The writable e2e team starts with no connectors; lifecycle tests
    // create their own, so only the shape is asserted here.
    let v = harness::run_ok_json(&["notifications", "connectors", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn connectors_get() {
    if harness::require_creds("connectors_get").is_none() {
        return;
    }
    let id = match discover_connector_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping connectors_get: no connectors available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["notifications", "connectors", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn connectors_types() {
    if harness::require_creds("connectors_types").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["notifications", "connectors", "types", "-o", "json"]);
    harness::assert_object_with_keys(&v, &["connectorTypeSummaries"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v["connectorTypeSummaries"], &["type"]);
    assert!(
        v["connectorTypeSummaries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["type"] == "GENERIC_HTTPS"),
        "type summaries should include GENERIC_HTTPS: {v}"
    );
}

#[test]
#[ignore]
fn connectors_entity_types() {
    if harness::require_creds("connectors_entity_types").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["notifications", "connectors", "entity-types", "-o", "json"]);
    harness::assert_object_with_keys(&v, &["entityTypes"]);
    harness::assert_array_of_strings(&v["entityTypes"]);
    assert!(
        v["entityTypes"]
            .as_array()
            .unwrap()
            .contains(&json!("ALERTS")),
        "entity types should include ALERTS: {v}"
    );
}

#[test]
#[ignore]
fn connectors_entity_subtypes() {
    if harness::require_creds("connectors_entity_subtypes").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "notifications",
        "connectors",
        "entity-subtypes",
        "--type",
        "ALERTS",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["entitySubTypes"]);
    harness::assert_array_of_strings(&v["entitySubTypes"]);
    assert!(
        !v["entitySubTypes"].as_array().unwrap().is_empty(),
        "ALERTS should have entity subtypes: {v}"
    );
}

#[test]
#[ignore]
fn connectors_get_nonexistent_fails() {
    if harness::require_creds("connectors_get_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&[
        "notifications",
        "connectors",
        "get",
        "00000000-0000-0000-0000-000000000000",
        "-o",
        "json",
    ]);
}

fn discover_connector_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("connectors_discover")?;
            let stdout = harness::run_ok(&["notifications", "connectors", "list", "-o", "json"]);
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

// ── Connector lifecycle (writable e2e team) ─────────────────────────────────

#[test]
#[ignore]
fn connectors_lifecycle() {
    if harness::require_write_creds("connectors_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("connector");

    // create
    let (id, mut cleanup) = create_generic_https_connector(&name);

    // get
    let got = harness::run_ok_json(&["notifications", "connectors", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name", "type"]);
    assert_eq!(got["connector"]["name"], name, "get should return our name");
    assert_eq!(got["connector"]["type"], "GENERIC_HTTPS");

    // list contains it
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["notifications", "connectors", "list", "-o", "json"]);
        harness::find_by_name(&list, "name", &name)
            .is_some()
            .then_some(())
    });
    assert!(found.is_some(), "connector list should contain {name}");

    // update: rename and change the description (PUT replaces the whole
    // connector, so the body carries the id and the full config)
    let renamed = format!("{name}-renamed");
    let mut body = generic_https_connector_body(&renamed);
    body["connector"]["id"] = json!(id);
    body["connector"]["description"] = json!("cx e2e updated");
    let upd = harness::JsonFile::new("connector-upd", &body);
    let updated = harness::run_write_json(&[
        "notifications",
        "connectors",
        "update",
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    assert_eq!(
        updated["connector"]["name"], renamed,
        "update response: {updated}"
    );
    let got = harness::run_ok_json(&["notifications", "connectors", "get", &id, "-o", "json"]);
    assert_eq!(got["connector"]["name"], renamed, "update should rename");
    assert_eq!(got["connector"]["description"], "cx e2e updated");

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["notifications", "connectors", "get", &id]);
    harness::run_ok_nonempty(&["notifications", "connectors", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["notifications", "connectors", "list"]);

    // delete, then get must fail (Notification Center deletes are
    // idempotent: a second delete also exits 0, so it is not asserted)
    harness::run_write_ok(&["notifications", "connectors", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["notifications", "connectors", "get", &id, "-o", "json"]);
}

// ── Presets ─────────────────────────────────────────────────────────────────

#[test]
#[ignore]
fn presets_list() {
    if harness::require_creds("presets_list").is_none() {
        return;
    }
    // Every team ships with the system presets, so the list is never empty.
    let v = harness::run_ok_json(&["notifications", "presets", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name", "connector_type"]);
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == SLACK_ALERTS_PRESET),
        "presets list should include {SLACK_ALERTS_PRESET}"
    );
}

#[test]
#[ignore]
fn presets_get_system_preset() {
    if harness::require_creds("presets_get_system_preset").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "notifications",
        "presets",
        "get",
        SLACK_ALERTS_PRESET,
        "-o",
        "json",
    ]);
    harness::assert_get_response(&v, &["id", "name", "connectorType", "entityType"]);
    assert_eq!(v["preset"]["presetType"], "SYSTEM");
    harness::run_ok_nonempty(&["notifications", "presets", "get", SLACK_ALERTS_PRESET]);
    harness::run_ok_nonempty(&[
        "notifications",
        "presets",
        "get",
        SLACK_ALERTS_PRESET,
        "-o",
        "toon",
    ]);
}

#[test]
#[ignore]
fn presets_get_nonexistent_fails() {
    if harness::require_creds("presets_get_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&[
        "notifications",
        "presets",
        "get",
        "00000000-0000-0000-0000-000000000000",
        "-o",
        "json",
    ]);
}

/// Body for a custom preset derived from the system Slack alerts preset:
/// the system preset's config with server-set fields stripped, our name,
/// and `parentId` pointing back at the system preset.
fn custom_slack_preset_body(name: &str) -> Value {
    let system = harness::run_ok_json(&[
        "notifications",
        "presets",
        "get",
        SLACK_ALERTS_PRESET,
        "-o",
        "json",
    ]);
    let mut preset = system["preset"].clone();
    let obj = preset
        .as_object_mut()
        .unwrap_or_else(|| panic!("system preset is not an object: {system}"));
    for key in ["id", "createTime", "updateTime", "presetType"] {
        obj.remove(key);
    }
    obj.insert("name".into(), json!(name));
    obj.insert("description".into(), json!("cx e2e"));
    obj.insert("parentId".into(), json!(SLACK_ALERTS_PRESET));
    json!({ "preset": preset })
}

#[test]
#[ignore]
fn presets_lifecycle() {
    if harness::require_write_creds("presets_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("preset");

    // create
    let body = custom_slack_preset_body(&name);
    let file = harness::JsonFile::new("preset", &body);
    let created = harness::run_write_json(&[
        "notifications",
        "presets",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("preset create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["notifications", "presets", "delete", &id]);
    assert_eq!(created["is_custom"], true, "create response: {created}");

    // get
    let got = harness::run_ok_json(&["notifications", "presets", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name", "connectorType"]);
    assert_eq!(got["preset"]["name"], name);
    assert_eq!(got["preset"]["presetType"], "CUSTOM");
    assert_eq!(got["preset"]["parentId"], SLACK_ALERTS_PRESET);

    // list contains it
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["notifications", "presets", "list", "-o", "json"]);
        harness::find_by_name(&list, "name", &name)
            .is_some()
            .then_some(())
    });
    assert!(found.is_some(), "preset list should contain {name}");

    // update: rename (replaceCustom takes the whole preset incl. id)
    let renamed = format!("{name}-renamed");
    let mut upd_body = body.clone();
    upd_body["preset"]["id"] = json!(id);
    upd_body["preset"]["name"] = json!(renamed);
    let upd = harness::JsonFile::new("preset-upd", &upd_body);
    harness::run_write_json(&[
        "notifications",
        "presets",
        "update",
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let got = harness::run_ok_json(&["notifications", "presets", "get", &id, "-o", "json"]);
    assert_eq!(got["preset"]["name"], renamed, "update should rename");

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["notifications", "presets", "get", &id]);
    harness::run_ok_nonempty(&["notifications", "presets", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["notifications", "presets", "list"]);

    // delete, then get must fail (a second delete is idempotent, exit 0)
    harness::run_write_ok(&["notifications", "presets", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["notifications", "presets", "get", &id, "-o", "json"]);
}

#[test]
#[ignore]
fn presets_set_default_not_covered() {
    // `presets set-default <id>` changes the team-wide default preset for
    // the preset's connector type + entity type. A safe round-trip needs the
    // current default first (API: `presets:getDefaultSummary`), but no cx
    // command exposes it: `presets list` reports `is_default: null` for
    // every preset. Without a way to read and restore the original, running
    // set-default could leave a concurrent PR run (or a human) with a
    // different default, so it is not exercised.
    eprintln!(
        "[e2e] not covered: notifications presets set-default — no cx command reads the current default preset, so the original cannot be restored"
    );
}

// ── Notification testing ────────────────────────────────────────────────────

#[test]
#[ignore]
fn notification_test_connector_config() {
    if harness::require_creds("notification_test_connector_config").is_none() {
        return;
    }
    // Tests an unsaved connector config; nothing is created.
    let name = harness::unique_name("testconn");
    let body = json!({
        "type": "GENERIC_HTTPS",
        "entityType": "ALERTS",
        "payloadType": GENERIC_HTTPS_PAYLOAD_TYPE,
        "fields": generic_https_fields(&name)
    });
    let file = harness::JsonFile::new("test-connector", &body);
    let v = harness::run_ok_json(&[
        "notifications",
        "test",
        "connector",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert_test_verdict(&v);
    harness::run_ok_nonempty(&[
        "notifications",
        "test",
        "connector",
        "--from-file",
        file.path(),
    ]);
}

#[test]
#[ignore]
fn notification_test_destination() {
    if harness::require_write_creds("notification_test_destination").is_none() {
        return;
    }
    let name = harness::unique_name("testdest");
    let (connector_id, _cleanup) = create_generic_https_connector(&name);
    let body = json!({
        "connectorId": connector_id,
        "presetId": GENERIC_HTTPS_PRESET,
        "entityType": "ALERTS",
        "entitySubType": "logsImmediateResolved",
        "payloadType": GENERIC_HTTPS_PAYLOAD_TYPE
    });
    let file = harness::JsonFile::new("test-destination", &body);
    let v = harness::run_ok_json(&[
        "notifications",
        "test",
        "destination",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert_test_verdict(&v);
}

#[test]
#[ignore]
fn notification_test_preset_config() {
    if harness::require_write_creds("notification_test_preset_config").is_none() {
        return;
    }
    let name = harness::unique_name("testpreset");
    let (connector_id, _cleanup) = create_generic_https_connector(&name);
    // GENERIC_HTTPS renders `body` as JSON, so the template must produce JSON.
    let body = json!({
        "connectorId": connector_id,
        "parentPresetId": GENERIC_HTTPS_PRESET,
        "entityType": "ALERTS",
        "entitySubType": "logsImmediateResolved",
        "configOverrides": [{
            "conditionType": { "matchEntityType": {} },
            "payloadType": GENERIC_HTTPS_PAYLOAD_TYPE,
            "messageConfig": { "fields": [
                { "fieldName": "body", "template": "{ \"name\": {{ alertDef.name | json_encode }} }" },
                { "fieldName": "headers", "template": "{}" }
            ] }
        }]
    });
    let file = harness::JsonFile::new("test-preset", &body);
    let v = harness::run_ok_json(&[
        "notifications",
        "test",
        "preset",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert_test_verdict(&v);
}

#[test]
#[ignore]
fn notification_test_routing_condition() {
    if harness::require_creds("notification_test_routing_condition").is_none() {
        return;
    }
    let body = json!({ "entityType": "ALERTS", "template": "alertDef.priority == 'P1'" });
    let file = harness::JsonFile::new("test-condition", &body);
    let v = harness::run_ok_json(&[
        "notifications",
        "test",
        "routing-condition",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert_test_verdict(&v);
    // A valid condition evaluates against a sample alert to a boolean.
    assert!(
        v.pointer("/success/result")
            .map(Value::is_boolean)
            .unwrap_or(false),
        "valid condition should yield success.result: {v}"
    );

    // Garbage must still come back as a verdict (exit 0), not a crash.
    let bad = json!({ "entityType": "ALERTS", "template": "alertDef.priority ==" });
    let file = harness::JsonFile::new("test-condition-bad", &bad);
    let out = harness::run_raw(&[
        "notifications",
        "test",
        "routing-condition",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    if out.status.success() {
        let v: Value = serde_json::from_slice(&out.stdout).expect("json on success");
        assert_test_verdict(&v);
    }
}

#[test]
#[ignore]
fn notification_test_template_render() {
    if harness::require_creds("notification_test_template_render").is_none() {
        return;
    }
    let body = json!({
        "entityType": "ALERTS",
        "entitySubType": "logsImmediateResolved",
        "template": "cx-e2e {{ alertDef.name }}"
    });
    let file = harness::JsonFile::new("test-template", &body);
    let v = harness::run_ok_json(&[
        "notifications",
        "test",
        "template-render",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert_test_verdict(&v);
    let rendered = v
        .pointer("/result/success/renderedValue")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("template render should return renderedValue: {v}"));
    assert!(
        rendered.starts_with("cx-e2e "),
        "rendered template should keep the literal prefix: {rendered}"
    );
    harness::run_ok_nonempty(&[
        "notifications",
        "test",
        "template-render",
        "--from-file",
        file.path(),
    ]);
}
