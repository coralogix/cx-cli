use crate::harness;
use serde_json::{json, Value};

/// Asserts values are populated, not just present (FORGE-710 rendered all-null rows).
#[test]
#[ignore]
fn suppression_rules_list() {
    if harness::require_creds("suppression_rules_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["alerts", "suppression-rules", "list", "-o", "json"]);
    let rows = harness::assert_array(&v);

    for (i, row) in rows.iter().enumerate() {
        let unique = row
            .get("unique_identifier")
            .unwrap_or(&serde_json::Value::Null);
        let name = row.get("name").unwrap_or(&serde_json::Value::Null);
        assert!(
            unique.is_string(),
            "row {i} has no unique_identifier - the list envelope is not being unwrapped: {row}"
        );
        assert!(
            name.is_string(),
            "row {i} has a null name - the list envelope is not being unwrapped: {row}"
        );
    }

    // Never-updated rules legitimately share both IDs, so only require one that differs.
    let distinguishes = rows.iter().any(|row| {
        let version = row.get("id").and_then(|v| v.as_str());
        version.is_some() && version != row.get("unique_identifier").and_then(|v| v.as_str())
    });
    assert!(
        rows.is_empty() || distinguishes,
        "every row reports the same value for both IDs, so the fixture can no longer \
         distinguish them"
    );
}

/// `get` must resolve the id that `list` hands out.
#[test]
#[ignore]
fn suppression_rules_get_resolves_the_id_list_reports() {
    if harness::require_creds("suppression_rules_get_resolves_the_id_list_reports").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["alerts", "suppression-rules", "list", "-o", "json"]);
    let rows = harness::assert_array(&v);
    let Some(id) = rows
        .iter()
        .find_map(|r| r.get("unique_identifier").and_then(|v| v.as_str()))
    else {
        eprintln!("skipping: the test team has no suppression rules to fetch");
        return;
    };

    let got = harness::run_ok_json(&["alerts", "suppression-rules", "get", id, "-o", "json"]);
    let rule = got
        .get("alertSchedulerRule")
        .unwrap_or_else(|| panic!("get returned no rule for the id list reported ({id}): {got}"));
    assert_eq!(
        rule.get("uniqueIdentifier").and_then(|v| v.as_str()),
        Some(id),
        "get returned a different rule than the one requested: {got}"
    );
}

/// Valid UUID shape so the backend treats it as unknown, not malformed.
const UNKNOWN_RULE_ID: &str = "00000000-0000-4000-8000-000000000000";

/// A 404 must become an empty result, not a raw API error.
#[test]
#[ignore]
fn suppression_rules_get_unknown_id_is_an_empty_result() {
    if harness::require_creds("suppression_rules_get_unknown_id_is_an_empty_result").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "alerts",
        "suppression-rules",
        "get",
        UNKNOWN_RULE_ID,
        "-o",
        "json",
    ]);
    assert!(
        harness::assert_array(&v).is_empty(),
        "get of an unknown id returned a rule: {v}"
    );
}

/// An unknown id must fail, not report a deletion. Gated as a write call.
#[test]
#[ignore]
fn suppression_rules_delete_unknown_id_fails() {
    if harness::require_write_creds("suppression_rules_delete_unknown_id_fails").is_none() {
        return;
    }
    let stderr = harness::run_fail(&[
        "alerts",
        "suppression-rules",
        "delete",
        UNKNOWN_RULE_ID,
        "--yes",
    ]);
    assert!(
        stderr.contains("No suppression rule found"),
        "expected the not-found guidance, stderr: {stderr}"
    );
}

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

/// A one-time MUTE rule scheduled far in the future, so it never affects any
/// real alert while it exists.
fn rule_body(name: &str) -> Value {
    json!({
        "alertSchedulerRule": {
            "name": name,
            "description": "cx e2e suppression rule; safe to delete",
            "enabled": true,
            "metaLabels": [],
            "filter": {
                "whatExpression": "source logs | filter true",
                "alertUniqueIds": { "value": [] }
            },
            "schedule": {
                "scheduleOperation": "SCHEDULE_OPERATION_MUTE",
                "oneTime": {
                    "timeframe": {
                        "startTime": "2036-01-01T00:00:00.000",
                        "endTime": "2036-01-01T01:00:00.000",
                        "timezone": "UTC"
                    }
                }
            }
        }
    })
}

/// create -> get -> list -> update -> text/toon -> delete -> get is empty.
#[test]
#[ignore]
fn suppression_rules_lifecycle() {
    if harness::require_write_creds("suppression_rules_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("supp");
    let body = rule_body(&name);

    // create: the CLI reports the stable uniqueIdentifier under `unique_identifier`
    let file = harness::JsonFile::new("supp", &body);
    let created = harness::run_write_json(&[
        "alerts",
        "suppression-rules",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = created["unique_identifier"]
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| panic!("create returned no unique_identifier: {created}"));
    let mut cleanup = harness::Cleanup::new(&["alerts", "suppression-rules", "delete", &id]);
    assert_eq!(created["name"], name);
    assert_eq!(created["enabled"], json!(true));

    // get resolves the uniqueIdentifier and echoes our fields
    let got = harness::run_ok_json(&["alerts", "suppression-rules", "get", &id, "-o", "json"]);
    let rule = got
        .get("alertSchedulerRule")
        .unwrap_or_else(|| panic!("get returned no alertSchedulerRule: {got}"));
    assert_eq!(rule["uniqueIdentifier"], id);
    assert_eq!(rule["name"], name);
    assert_eq!(
        rule.pointer("/schedule/scheduleOperation"),
        Some(&json!("SCHEDULE_OPERATION_MUTE"))
    );

    // list contains ours, keyed by the stable id
    let list = harness::run_ok_json(&["alerts", "suppression-rules", "list", "-o", "json"]);
    let listed = harness::find_by_name(&list, "name", &name)
        .unwrap_or_else(|| panic!("suppression-rules list should contain {name}"));
    assert_eq!(listed["unique_identifier"], id);

    // update: must identify the rule by uniqueIdentifier; flips enabled and description
    let mut updated = body.clone();
    updated["alertSchedulerRule"]["uniqueIdentifier"] = json!(id);
    updated["alertSchedulerRule"]["description"] = json!("cx e2e updated");
    updated["alertSchedulerRule"]["enabled"] = json!(false);
    let upd = harness::JsonFile::new("supp-upd", &updated);
    let upd_resp = harness::run_write_json(&[
        "alerts",
        "suppression-rules",
        "update",
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    assert_eq!(
        upd_resp["unique_identifier"], id,
        "update must keep the stable id: {upd_resp}"
    );
    let got = harness::run_ok_json(&["alerts", "suppression-rules", "get", &id, "-o", "json"]);
    let rule = &got["alertSchedulerRule"];
    assert_eq!(
        rule["description"], "cx e2e updated",
        "update not visible: {got}"
    );
    assert_eq!(rule["enabled"], json!(false), "update not visible: {got}");
    assert_eq!(rule["uniqueIdentifier"], id);

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["alerts", "suppression-rules", "get", &id]);
    harness::run_ok_nonempty(&["alerts", "suppression-rules", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["alerts", "suppression-rules", "list"]);

    // delete; afterwards get is an empty result (404 -> []), and a second
    // delete fails with the not-found guidance
    harness::run_write_ok(&["alerts", "suppression-rules", "delete", &id]);
    cleanup.disarm();
    let after = harness::run_ok_json(&["alerts", "suppression-rules", "get", &id, "-o", "json"]);
    assert!(
        harness::assert_array(&after).is_empty(),
        "get after delete should be empty: {after}"
    );
    let stderr = harness::run_fail(&["alerts", "suppression-rules", "delete", &id, "--yes"]);
    assert!(
        stderr.contains("No suppression rule found"),
        "second delete should report not found: {stderr}"
    );
}
