use crate::harness;
use serde_json::{json, Value};

#[test]
#[ignore]
fn e2m_list() {
    if harness::require_creds("e2m_list").is_none() {
        return;
    }
    // The writable e2e team starts with no E2M definitions; the lifecycle
    // test below creates its own, so only the shape is asserted.
    let v = harness::run_ok_json(&["e2m", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn e2m_limits() {
    if harness::require_creds("e2m_limits").is_none() {
        return;
    }
    // `limits` prints one row per profile, so the JSON is an array.
    let v = harness::run_ok_json(&["e2m", "limits", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["limit", "used", "labels_limit"]);
    let row = &v[0];
    assert!(
        row["limit"].as_u64().is_some(),
        "limit should be the team's E2M definition cap: {row}"
    );
    harness::run_ok_nonempty(&["e2m", "limits"]);
    harness::run_ok_nonempty(&["e2m", "limits", "-o", "toon"]);
}

#[test]
#[ignore]
fn e2m_labels_cardinality() {
    if harness::require_creds("e2m_labels_cardinality").is_none() {
        return;
    }
    // The CLI takes no draft labels, so the API answers an empty list; the
    // call itself must succeed.
    let v = harness::run_ok_json(&["e2m", "labels-cardinality", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["day", "permutations"]);
}

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

/// A logs-to-metrics definition filtered to an application that does not
/// exist (our unique name), so it never produces a metric series.
fn e2m_body(name: &str, description: &str) -> Value {
    let metric = name.replace('-', "_");
    json!({
        "name": name,
        "description": description,
        "permutationsLimit": 100,
        "type": "E2M_TYPE_LOGS2METRICS",
        "logsQuery": {
            "lucene": format!("coralogix.metadata.applicationName:\"{name}\""),
            "applicationnameFilters": [name],
            "subsystemnameFilters": [],
            "severityFilters": []
        },
        "metricLabels": [
            { "targetLabel": "app", "sourceField": "coralogix.metadata.applicationName" }
        ],
        "metricFields": [
            {
                "targetBaseMetricName": metric,
                "sourceField": "log_obj.cx_e2e_value",
                "aggregations": [
                    { "enabled": true, "aggType": "AGG_TYPE_COUNT", "targetMetricName": format!("{metric}_count") }
                ]
            }
        ]
    })
}

/// `get` wraps the definition as `{"e2m": {...}}`.
fn e2m_of(get_response: &Value) -> &Value {
    get_response.get("e2m").unwrap_or(get_response)
}

#[test]
#[ignore]
fn e2m_lifecycle() {
    if harness::require_write_creds("e2m_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("e2m");

    // create
    let file = harness::JsonFile::new("e2m", &e2m_body(&name, "e2e definition"));
    let created =
        harness::run_write_json(&["e2m", "create", "--from-file", file.path(), "-o", "json"]);
    assert_eq!(created["name"], name, "create should echo our name");
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("e2m create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["e2m", "delete", &id]);

    // get
    let got = harness::run_ok_json(&["e2m", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name"]);
    let def = e2m_of(&got);
    assert_eq!(def["name"], name);
    assert_eq!(
        def.pointer("/logsQuery/applicationnameFilters/0"),
        Some(&json!(name)),
        "definition must stay scoped to our application: {got}"
    );

    // list contains it
    let list = harness::run_ok_json(&["e2m", "list", "-o", "json"]);
    assert!(
        harness::find_by_name(&list, "name", &name).is_some(),
        "e2m list should contain {name}"
    );

    // update: PUT with no path id, so the body carries `id`.
    // `permutationsLimit` is create-only; the API rejects it on update.
    let mut updated = e2m_body(&name, "e2e definition updated");
    updated["id"] = json!(id);
    updated.as_object_mut().unwrap().remove("permutationsLimit");
    let upd = harness::JsonFile::new("e2m-upd", &updated);
    harness::run_write_ok(&["e2m", "update", "--from-file", upd.path(), "-o", "json"]);
    let got = harness::run_ok_json(&["e2m", "get", &id, "-o", "json"]);
    assert_eq!(
        e2m_of(&got)["description"],
        json!("e2e definition updated"),
        "update should change the description: {got}"
    );

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["e2m", "get", &id]);
    harness::run_ok_nonempty(&["e2m", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["e2m", "list"]);
    harness::run_ok_nonempty(&["e2m", "list", "-o", "toon"]);

    // delete, then get must fail
    harness::run_write_ok(&["e2m", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["e2m", "get", &id, "-o", "json"]);
}

#[test]
#[ignore]
fn e2m_get_and_delete_missing_fail() {
    if harness::require_write_creds("e2m_get_and_delete_missing_fail").is_none() {
        return;
    }
    let bogus = "00000000-0000-4000-8000-000000000000";
    harness::run_fail(&["e2m", "get", bogus, "-o", "json"]);
    harness::run_fail(&["e2m", "delete", bogus, "--yes"]);
}
