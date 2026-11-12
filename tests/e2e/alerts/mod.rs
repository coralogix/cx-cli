use std::sync::OnceLock;

use crate::harness;
use serde_json::{json, Value};

/// Valid UUID shape so the backend treats it as unknown, not malformed.
const UNKNOWN_ALERT_ID: &str = "00000000-0000-4000-8000-000000000000";

#[test]
#[ignore]
fn alerts_list() {
    if harness::require_creds("alerts_list").is_none() {
        return;
    }
    // The e2e team carries seed "[otel-demo] ..." alerts, so non-empty is safe.
    let v = harness::run_ok_json(&["alerts", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name", "enabled"]);
}

#[test]
#[ignore]
fn alerts_get() {
    if harness::require_creds("alerts_get").is_none() {
        return;
    }
    let id = match discover_alert_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping alerts_get: no alerts available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["alerts", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["alertDef"]);
}

/// `get` of an unknown id must fail, not print an empty alert.
#[test]
#[ignore]
fn alerts_get_unknown_id_fails() {
    if harness::require_creds("alerts_get_unknown_id_fails").is_none() {
        return;
    }
    harness::run_fail(&["alerts", "get", UNKNOWN_ALERT_ID, "-o", "json"]);
}

/// Generic event listing (no version filter) over a short window. The team
/// may have no events in the window, so only the shape is asserted.
#[test]
#[ignore]
fn alerts_events_recent() {
    if harness::require_creds("alerts_events_recent").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "alerts", "events", "--start", "now-1h", "--end", "now", "-o", "json",
    ]);
    harness::assert_array(&v);
}

/// Event stats is a team-wide read; it may legitimately be empty.
#[test]
#[ignore]
fn alerts_event_stats() {
    if harness::require_creds("alerts_event_stats").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["alerts", "event-stats", "-o", "json"]);
    harness::assert_array(&v);
    harness::run_ok(&["alerts", "event-stats"]);
}

fn discover_alert_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("alerts_discover")?;
            let stdout = harness::run_ok(&["alerts", "list", "-o", "json"]);
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

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

/// A logs-threshold alert that can never notify: created disabled, in phantom
/// mode, with no destinations, a nonsense Lucene query and a huge threshold.
/// The API requires `rules[].override` even though it is documented as
/// optional, so it is always set.
fn alert_body(name: &str) -> Value {
    json!({
        "alertDefProperties": {
            "name": name,
            "description": "cx e2e alert; safe to delete",
            "enabled": false,
            "phantomMode": true,
            "priority": "ALERT_DEF_PRIORITY_P5_OR_UNSPECIFIED",
            "type": "ALERT_DEF_TYPE_LOGS_THRESHOLD",
            "groupByKeys": [],
            "entityLabels": {},
            "incidentsSettings": {
                "minutes": 30,
                "notifyOn": "NOTIFY_ON_TRIGGERED_ONLY_UNSPECIFIED"
            },
            "notificationGroup": { "groupByKeys": [], "webhooks": [], "destinations": [] },
            "logsThreshold": {
                "logsFilter": {
                    "simpleFilter": {
                        "luceneQuery": "cx_e2e_never_matches_7f3a",
                        "labelFilters": {
                            "applicationName": [
                                {
                                    "value": "otel-demo",
                                    "operation": "LOG_FILTER_OPERATION_TYPE_IS_OR_UNSPECIFIED"
                                }
                            ],
                            "subsystemName": [],
                            "severities": ["LOG_SEVERITY_CRITICAL"]
                        }
                    }
                },
                "rules": [
                    {
                        "condition": {
                            "threshold": 1000000,
                            "timeWindow": {
                                "logsTimeWindowSpecificValue":
                                    "LOGS_TIME_WINDOW_VALUE_MINUTES_5_OR_UNSPECIFIED"
                            },
                            "conditionType":
                                "LOGS_THRESHOLD_CONDITION_TYPE_MORE_THAN_OR_UNSPECIFIED"
                        },
                        "override": { "priority": "ALERT_DEF_PRIORITY_P5_OR_UNSPECIFIED" }
                    }
                ],
                "notificationPayloadFilter": []
            }
        }
    })
}

fn get_alert(id: &str) -> Value {
    harness::run_ok_json(&["alerts", "get", id, "-o", "json"])
}

fn alert_enabled(v: &Value) -> Option<bool> {
    v.pointer("/alertDef/alertDefProperties/enabled")
        .and_then(Value::as_bool)
}

/// create -> get -> list --name -> enable/disable -> get by version id ->
/// events for that version -> text/toon -> delete -> get fails.
#[test]
#[ignore]
fn alerts_lifecycle() {
    if harness::require_write_creds("alerts_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("alert");

    // create
    let file = harness::JsonFile::new("alert", &alert_body(&name));
    let created =
        harness::run_write_json(&["alerts", "create", "--from-file", file.path(), "-o", "json"]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("alert create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["alerts", "delete", &id]);
    assert_eq!(created["name"], name);
    assert_eq!(created["enabled"], json!(false));

    // get: full definition, our fields, created disabled and phantom
    let got = get_alert(&id);
    harness::assert_get_response(&got, &["alertDef"]);
    assert_eq!(got["alertDef"]["id"], id);
    assert_eq!(
        got.pointer("/alertDef/alertDefProperties/name"),
        Some(&json!(name))
    );
    assert_eq!(alert_enabled(&got), Some(false), "created disabled: {got}");
    assert_eq!(
        got.pointer("/alertDef/alertDefProperties/phantomMode"),
        Some(&json!(true)),
        "phantom mode should be preserved: {got}"
    );
    let version_id = got["alertDef"]["alertVersionId"]
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| panic!("alert get returned no alertVersionId: {got}"));

    // list --name filters to ours (substring match on the unique name)
    let list = harness::run_ok_json(&["alerts", "list", "--name", &name, "-o", "json"]);
    let rows = harness::assert_array(&list);
    assert_eq!(rows.len(), 1, "list --name should match only ours: {list}");
    assert_eq!(rows[0]["id"], id);
    let full = harness::run_ok_json(&["alerts", "list", "-o", "json"]);
    assert!(
        harness::find_by_name(&full, "name", &name).is_some(),
        "alerts list should contain {name}"
    );

    // enable, then disable: `enabled` must flip on get each time
    harness::run_write_ok(&["alerts", "enable", &id]);
    assert_eq!(
        alert_enabled(&get_alert(&id)),
        Some(true),
        "enable did not stick"
    );
    harness::run_write_ok(&["alerts", "disable", &id]);
    assert_eq!(
        alert_enabled(&get_alert(&id)),
        Some(false),
        "disable did not stick"
    );

    // get accepts the alert version id too (404 on def id -> version lookup)
    let by_version = get_alert(&version_id);
    assert_eq!(
        by_version["alertDef"]["id"], id,
        "get by version id should resolve the same alert: {by_version}"
    );

    // events scoped to our version: never fires, so an empty array is the contract
    let events = harness::run_ok_json(&[
        "alerts",
        "events",
        "--alert-version-id",
        &version_id,
        "--start",
        "now-1h",
        "--end",
        "now",
        "-o",
        "json",
    ]);
    assert!(
        harness::assert_array(&events).is_empty(),
        "a phantom alert with an impossible query must have no events: {events}"
    );
    harness::run_ok(&["alerts", "events", "--alert-version-id", &version_id]);

    // text + toon renderers on a known resource
    let text = harness::run_ok_nonempty(&["alerts", "get", &id]);
    assert!(
        String::from_utf8_lossy(&text).contains(&name),
        "text get should show the alert name"
    );
    harness::run_ok_nonempty(&["alerts", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["alerts", "list", "--name", &name]);
    harness::run_ok_nonempty(&["alerts", "list", "--name", &name, "-o", "toon"]);

    // delete, then get must fail
    harness::run_write_ok(&["alerts", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["alerts", "get", &id, "-o", "json"]);
}

/// Write verbs on an unknown id must fail loudly.
#[test]
#[ignore]
fn alerts_write_verbs_unknown_id_fail() {
    if harness::require_write_creds("alerts_write_verbs_unknown_id_fail").is_none() {
        return;
    }
    for verb in ["delete", "enable", "disable"] {
        let stderr = harness::run_fail(&["alerts", verb, UNKNOWN_ALERT_ID, "--yes"]);
        assert!(
            stderr.contains("404") || stderr.to_lowercase().contains("not found"),
            "`alerts {verb}` on an unknown id should report not found: {stderr}"
        );
    }
}

/// `create` rejects a body without `alertDefProperties` before calling the API.
#[test]
#[ignore]
fn alerts_create_rejects_bad_body() {
    if harness::require_write_creds("alerts_create_rejects_bad_body").is_none() {
        return;
    }
    let file = harness::JsonFile::new("alert-bad", &json!({ "name": "no properties" }));
    let stderr = harness::run_fail(&["alerts", "create", "--from-file", file.path(), "--yes"]);
    assert!(
        stderr.contains("alertDefProperties"),
        "expected the validation message, stderr: {stderr}"
    );
}
