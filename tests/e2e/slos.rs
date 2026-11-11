use std::sync::OnceLock;

use crate::harness;
use serde_json::{json, Value};

/// Valid UUID shape so the backend treats it as unknown, not malformed.
const UNKNOWN_SLO_ID: &str = "00000000-0000-4000-8000-000000000000";

#[test]
#[ignore]
fn slos_list() {
    if harness::require_creds("slos_list").is_none() {
        return;
    }
    // The writable e2e team starts with no SLOs; the lifecycle test below
    // creates its own, so only the shape is asserted here.
    let v = harness::run_ok_json(&["slos", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name", "target"]);
}

#[test]
#[ignore]
fn slos_get() {
    if harness::require_creds("slos_get").is_none() {
        return;
    }
    let id = match discover_slo_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping slos_get: no SLOs available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["slos", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["slo"]);
}

/// `get` of an unknown id must fail, not print an empty SLO.
#[test]
#[ignore]
fn slos_get_unknown_id_fails() {
    if harness::require_creds("slos_get_unknown_id_fails").is_none() {
        return;
    }
    let stderr = harness::run_fail(&["slos", "get", UNKNOWN_SLO_ID, "-o", "json"]);
    assert!(
        stderr.contains("404") || stderr.to_lowercase().contains("not found"),
        "expected a not-found error: {stderr}"
    );
}

fn discover_slo_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("slos_discover")?;
            let stdout = harness::run_ok(&["slos", "list", "-o", "json"]);
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

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

/// The API returns whole-number targets as integers (`99`, not `99.0`), so
/// compare as floats.
fn assert_target(v: &Value, expected: f64, what: &str) {
    assert_eq!(
        v.as_f64(),
        Some(expected),
        "{what}: expected target {expected}, got {v}"
    );
}

/// A request-based metric SLO over the otel-demo span metrics. The create
/// body is the bare SLO object (no `slo` wrapper); the server derives
/// `sloType` and `productType`.
fn slo_body(name: &str) -> Value {
    json!({
        "name": name,
        "description": "cx e2e SLO; safe to delete",
        "sloTimeFrame": "SLO_TIME_FRAME_7_DAYS",
        "targetThresholdPercentage": 99.0,
        "requestBasedMetricSli": {
            "goodEvents": {
                "query": "sum(rate(calls_total{status_code!=\"STATUS_CODE_ERROR\"}[1m]))"
            },
            "totalEvents": { "query": "sum(rate(calls_total[1m]))" }
        }
    })
}

/// create -> get -> list -> update (replace from the `get` body) ->
/// text/toon -> delete -> get fails.
#[test]
#[ignore]
fn slos_lifecycle() {
    if harness::require_write_creds("slos_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("slo");

    // create
    let file = harness::JsonFile::new("slo", &slo_body(&name));
    let created =
        harness::run_write_json(&["slos", "create", "--from-file", file.path(), "-o", "json"]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("slo create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["slos", "delete", &id]);
    assert_eq!(created["name"], name);
    assert_target(&created["target"], 99.0, "create response");
    assert_eq!(created["type"], "REQUEST");

    // get: raw API shape under `slo`, with our fields and the server-derived type
    let got = harness::run_ok_json(&["slos", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["slo"]);
    let slo = &got["slo"];
    assert_eq!(slo["id"], id);
    assert_eq!(slo["name"], name);
    assert_eq!(slo["sloTimeFrame"], "SLO_TIME_FRAME_7_DAYS");
    assert_eq!(slo["sloType"], "SLO_TYPE_REQUEST");
    assert_target(&slo["targetThresholdPercentage"], 99.0, "get after create");
    assert!(
        slo.get("requestBasedMetricSli").is_some(),
        "get should echo the SLI: {got}"
    );

    // list contains ours
    let list = harness::run_ok_json(&["slos", "list", "-o", "json"]);
    let listed = harness::find_by_name(&list, "name", &name)
        .unwrap_or_else(|| panic!("slos list should contain {name}"));
    assert_eq!(listed["id"], id);

    // update: replace from the `get` body (round-trip), changing two fields
    let mut updated = slo.clone();
    updated["description"] = json!("cx e2e updated");
    updated["targetThresholdPercentage"] = json!(98.5);
    let upd = harness::JsonFile::new("slo-upd", &updated);
    let upd_resp =
        harness::run_write_json(&["slos", "update", "--from-file", upd.path(), "-o", "json"]);
    assert_eq!(upd_resp["id"], id, "update must keep the id: {upd_resp}");
    assert_target(&upd_resp["target"], 98.5, "update response");
    let got = harness::run_ok_json(&["slos", "get", &id, "-o", "json"]);
    assert_eq!(
        got["slo"]["description"], "cx e2e updated",
        "update not visible on get: {got}"
    );
    assert_target(
        &got["slo"]["targetThresholdPercentage"],
        98.5,
        "get after update",
    );
    assert_eq!(
        got.pointer("/slo/revision/revision"),
        Some(&json!(2)),
        "a replace should bump the revision: {got}"
    );

    // text + toon renderers on a known resource
    let text = harness::run_ok_nonempty(&["slos", "get", &id]);
    assert!(
        String::from_utf8_lossy(&text).contains(&name),
        "text get should show the SLO name"
    );
    harness::run_ok_nonempty(&["slos", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["slos", "list"]);
    harness::run_ok_nonempty(&["slos", "list", "-o", "toon"]);

    // delete, then get must fail; a second delete must fail too
    harness::run_write_ok(&["slos", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["slos", "get", &id, "-o", "json"]);
    harness::run_fail(&["slos", "delete", &id, "--yes"]);
}

/// `delete` of an unknown id must fail, not report a deletion.
#[test]
#[ignore]
fn slos_delete_unknown_id_fails() {
    if harness::require_write_creds("slos_delete_unknown_id_fails").is_none() {
        return;
    }
    let stderr = harness::run_fail(&["slos", "delete", UNKNOWN_SLO_ID, "--yes"]);
    assert!(
        stderr.contains("404") || stderr.to_lowercase().contains("not found"),
        "expected a not-found error: {stderr}"
    );
}
