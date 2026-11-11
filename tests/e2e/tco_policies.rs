use std::time::Duration;

use crate::harness;
use serde_json::{json, Value};

const NONEXISTENT_POLICY_ID: &str = "00000000-0000-0000-0000-000000000000";

#[test]
#[ignore]
fn tco_policies_list() {
    if harness::require_creds("tco_policies_list").is_none() {
        return;
    }
    // The writable e2e team starts with no TCO policies; the lifecycle test
    // creates its own, so only the shape is asserted here.
    let v = harness::run_ok_json(&["tco", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name", "priority"]);
}

#[test]
#[ignore]
fn tco_policies_get_nonexistent_fails() {
    if harness::require_creds("tco_policies_get_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&["tco", "get", NONEXISTENT_POLICY_ID, "-o", "json"]);
}

#[test]
#[ignore]
fn tco_policies_settings() {
    if harness::require_creds("tco_policies_settings").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["tco", "settings", "-o", "json"]);
    harness::assert_object_with_keys(&v, &["logsPolicySettings", "spansPolicySettings"]);
    harness::run_ok_nonempty(&["tco", "settings"]);
    harness::run_ok_nonempty(&["tco", "settings", "-o", "toon"]);
}

#[test]
#[ignore]
fn tco_policies_settings_update_roundtrip() {
    if harness::require_write_creds("tco_policies_settings_update_roundtrip").is_none() {
        return;
    }
    let before = harness::run_ok_json(&["tco", "settings", "-o", "json"]);
    harness::assert_object_with_keys(&before, &["logsPolicySettings", "spansPolicySettings"]);
    let file = harness::JsonFile::new("tco-settings", &before);
    let v = harness::run_write_json(&[
        "tco",
        "settings-update",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["logsPolicySettings", "spansPolicySettings"]);
    let after = harness::run_ok_json(&["tco", "settings", "-o", "json"]);
    assert_eq!(
        after, before,
        "writing the same settings back must not change them"
    );
}

fn policy_body(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "priority": "PRIORITY_TYPE_LOW",
        // Matches only an application named after this run, so it never
        // touches the live otel-demo data.
        "applicationRule": { "name": name, "ruleTypeId": "RULE_TYPE_ID_IS" },
        "logRules": { "severities": ["SEVERITY_DEBUG"] }
    })
}

fn create_policy(name: &str) -> String {
    let file = harness::JsonFile::new("tco", &policy_body(name, "cx e2e policy"));
    let created =
        harness::run_write_json(&["tco", "create", "--from-file", file.path(), "-o", "json"]);
    assert_eq!(created["name"], name);
    harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("tco create returned no id: {created}"))
}

/// `tco get` wraps the policy in `policy`.
fn get_policy(id: &str) -> Value {
    let v = harness::run_ok_json(&["tco", "get", id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name", "priority"]);
    v["policy"].clone()
}

/// Every logs policy of the team as `{id, order}`, in its current order.
/// Other test runs may create or delete policies concurrently, so callers
/// retry on failure.
fn logs_policy_orders() -> Vec<Value> {
    let list = harness::run_ok_json(&["tco", "list", "-o", "json"]);
    let mut policies: Vec<(i64, String)> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["id"].as_str().map(str::to_string))
        .filter_map(|id| {
            let out = harness::run_raw(&["tco", "get", &id, "-o", "json"]);
            if !out.status.success() {
                return None; // deleted by a parallel run meanwhile
            }
            let policy = harness::parse_json(&out.stdout)?["policy"].clone();
            let is_logs = policy["targets"]
                .as_array()
                .is_some_and(|t| t.iter().any(|x| x["dataset"] == "logs"))
                || policy.get("logRules").is_some();
            is_logs.then(|| (policy["order"].as_i64().unwrap_or(i64::MAX), id))
        })
        .collect();
    policies.sort();
    policies
        .into_iter()
        .enumerate()
        .map(|(i, (_, id))| json!({ "id": id, "order": i + 1 }))
        .collect()
}

#[test]
#[ignore]
fn tco_policies_lifecycle() {
    if harness::require_write_creds("tco_policies_lifecycle").is_none() {
        return;
    }
    let name_a = harness::unique_name("tco");
    let name_b = harness::unique_name("tco");

    // create two policies (reorder needs more than one of ours)
    let id_a = create_policy(&name_a);
    let mut cleanup_a = harness::Cleanup::new(&["tco", "delete", &id_a]);
    let id_b = create_policy(&name_b);
    let mut cleanup_b = harness::Cleanup::new(&["tco", "delete", &id_b]);

    // get
    let got = get_policy(&id_a);
    assert_eq!(got["id"], id_a);
    assert_eq!(got["name"], name_a);
    assert_eq!(got["priority"], "PRIORITY_TYPE_LOW");
    assert_eq!(got["applicationRule"]["name"], name_a);

    // list contains both
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["tco", "list", "-o", "json"]);
        (harness::find_by_name(&list, "name", &name_a).is_some()
            && harness::find_by_name(&list, "name", &name_b).is_some())
        .then_some(())
    });
    assert!(
        found.is_some(),
        "tco list should contain {name_a} and {name_b}"
    );

    // update: rename A (update takes the full definition plus id)
    let renamed = format!("{name_a}-renamed");
    let mut body = policy_body(&name_a, "cx e2e policy updated");
    body["id"] = json!(id_a);
    body["name"] = json!(renamed);
    body["enabled"] = json!(true);
    let upd = harness::JsonFile::new("tco-upd", &body);
    let updated =
        harness::run_write_json(&["tco", "update", "--from-file", upd.path(), "-o", "json"]);
    assert_eq!(updated["name"], renamed);
    let got = get_policy(&id_a);
    assert_eq!(got["name"], renamed, "tco update should rename");
    assert_eq!(got["description"], "cx e2e policy updated");

    // reorder: keep every logs policy in its current order, but swap ours so
    // B comes before A. Reordering is team-global, so a parallel run's
    // reorder can land between ours and the read-back and undo the swap; the
    // whole reorder-then-verify step is retried until the order sticks.
    let reordered = harness::eventually(5, Duration::from_secs(2), || {
        let mut orders = logs_policy_orders();
        let pos_a = orders.iter().position(|o| o["id"] == id_a)?;
        let pos_b = orders.iter().position(|o| o["id"] == id_b)?;
        if pos_a < pos_b {
            orders.swap(pos_a, pos_b);
            for (i, o) in orders.iter_mut().enumerate() {
                o["order"] = json!(i + 1);
            }
        }
        let file = harness::JsonFile::new(
            "tco-reorder",
            &json!({ "sourceType": "SOURCE_TYPE_LOGS", "orders": orders }),
        );
        let out = harness::run_raw(&[
            "tco",
            "reorder",
            "--from-file",
            file.path(),
            "-o",
            "json",
            "--yes",
        ]);
        if !out.status.success() {
            return None;
        }
        let order_a = get_policy(&id_a)["order"].as_i64().unwrap_or(i64::MAX);
        let order_b = get_policy(&id_b)["order"].as_i64().unwrap_or(i64::MAX);
        (order_b < order_a).then_some((order_a, order_b))
    });
    assert!(
        reordered.is_some(),
        "after reorder B should precede A (team-global order kept flipping back)"
    );

    // test: evaluate the policies against sample log metadata
    let test = harness::JsonFile::new(
        "tco-test",
        &json!({ "metaFieldsValuesList": [{
            "applicationNameValues": name_a,
            "subsystemNameValues": "cx-e2e",
            "severityValues": "debug"
        }] }),
    );
    let result = harness::run_ok_json(&["tco", "test", "--from-file", test.path(), "-o", "json"]);
    let results = result["testPoliciesBulkResult"]
        .as_array()
        .unwrap_or_else(|| panic!("tco test should return testPoliciesBulkResult: {result}"));
    assert_eq!(results.len(), 1);
    assert!(
        results[0]["matched"].is_boolean(),
        "result has matched flag: {result}"
    );

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["tco", "get", &id_a]);
    harness::run_ok_nonempty(&["tco", "get", &id_a, "-o", "toon"]);

    // delete both, then get must fail
    harness::run_write_ok(&["tco", "delete", &id_a]);
    cleanup_a.disarm();
    harness::run_write_ok(&["tco", "delete", &id_b]);
    cleanup_b.disarm();
    harness::run_fail(&["tco", "get", &id_a, "-o", "json"]);
    harness::run_fail(&["tco", "get", &id_b, "-o", "json"]);
    harness::run_fail(&["tco", "delete", NONEXISTENT_POLICY_ID, "--yes"]);
}
