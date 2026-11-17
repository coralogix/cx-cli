use crate::harness;

#[test]
#[ignore]
fn datasets_list() {
    if harness::require_creds("datasets_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["datasets", "list", "-o", "json"]);
    // The response is always an array of dataset rows; the test team always
    // has at least the `default/logs` system dataset.
    harness::assert_nonempty_array_of_objects_with_keys(
        &v,
        &[
            "teamId",
            "dataspace",
            "dataset",
            "version",
            "pillar",
            "entityType",
        ],
    );
}

#[test]
#[ignore]
fn datasets_list_with_filter() {
    if harness::require_creds("datasets_list_with_filter").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "datasets",
        "list",
        "--filter",
        "<v1> $d.pillar == 'logs'",
        "-o",
        "json",
    ]);
    let arr = harness::assert_array(&v);
    for item in arr {
        assert_eq!(
            item["pillar"].as_str(),
            Some("logs"),
            "filtered list must only contain logs-pillar datasets"
        );
    }
}

/// `cx datasets fields` against the real Schema Store keypaths search: every
/// team's logs carry the `$l` labels, so a `$l` prefix search is never empty.
#[test]
#[ignore]
fn datasets_fields_prefix() {
    if harness::require_creds("datasets_fields_prefix").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["datasets", "fields", "--prefix", "$l", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["keypath", "types"]);
    for row in harness::assert_array(&v) {
        let keypath = row["keypath"].as_str().unwrap_or_default();
        assert!(
            keypath.starts_with("$l"),
            "--prefix '$l' returned {keypath}"
        );
    }
}

/// `cx datasets field-values` against the real keypaths samples endpoint.
/// Rows are `{"value": ...}`; the list may be empty on a quiet team.
#[test]
#[ignore]
fn datasets_field_values() {
    if harness::require_creds("datasets_field_values").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "datasets",
        "field-values",
        "$l.applicationName",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, &["value"]);
}

/// The server caps the time range at 7 days; a wider range must fail with
/// the API's 400, not succeed with partial results.
#[test]
#[ignore]
fn datasets_fields_rejects_range_over_seven_days() {
    if harness::require_creds("datasets_fields_rejects_range_over_seven_days").is_none() {
        return;
    }
    let stderr = harness::run_fail(&[
        "datasets", "fields", "--prefix", "$l", "--start", "now-8d", "-o", "json",
    ]);
    // Match the API error, not any 400 (an OAuth refresh failure is also a 400).
    assert!(
        stderr.contains("API request failed (400"),
        "expected an API 400 for an 8-day range, got: {stderr}"
    );
    eprintln!("[e2e] 8-day range rejected with: {}", stderr.trim());
}
