use crate::harness;

/// Guards the FORGE-710 regression against the live API.
///
/// The bug that motivated this test was invisible to a key-presence check:
/// `list` returned the right *number* of rows with every value deserialized to
/// `null`, and exited 0. So assert the values are actually populated, and that
/// each row carries both IDs as distinct values - conflating them is what broke
/// console links, `get`, `update` and `delete`.
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
        // Both IDs are reported, and they are genuinely different values.
        if let Some(version) = row.get("id").and_then(|v| v.as_str()) {
            assert_ne!(
                Some(version),
                unique.as_str(),
                "row {i} reports the same value for both IDs, so the fixture can no longer \
                 distinguish them: {row}"
            );
        }
    }
}

/// `get` must resolve the id that `list` hands out. Before FORGE-710 `list`
/// surfaced the rule *version* id, which `get` answers with an empty body.
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

/// A well-formed id that no rule carries. Shaped like a v4 UUID so the backend
/// treats it as an unknown rule rather than a malformed one.
const UNKNOWN_RULE_ID: &str = "00000000-0000-4000-8000-000000000000";

/// Since CX-57145 the backend answers an unknown id with 404. `get` must turn
/// that into an empty result, not a raw "API request failed (404)" -
/// `run_ok_json` fails on either a non-zero exit or an API error on stderr.
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

/// The live check of the CX-57145 contract `delete` now relies on: an unknown
/// id must fail, not report a deletion. Before the fix the backend answered
/// 200 here without deleting anything. The id matches no rule, so nothing is
/// deleted either way, but it is still a write call and gated like the rest.
#[test]
#[ignore]
fn suppression_rules_delete_unknown_id_fails() {
    if std::env::var_os("CX_E2E_INCLUDE_WRITES").is_none() {
        return;
    }
    if harness::require_creds("suppression_rules_delete_unknown_id_fails").is_none() {
        return;
    }
    let output = harness::cx()
        .args([
            "alerts",
            "suppression-rules",
            "delete",
            UNKNOWN_RULE_ID,
            "--yes",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "deleting an unknown id reported success - is the backend 404 (CX-57145) live? \
         stderr: {stderr}"
    );
    assert!(
        stderr.contains("No suppression rule found"),
        "expected the not-found guidance, stderr: {stderr}"
    );
}
