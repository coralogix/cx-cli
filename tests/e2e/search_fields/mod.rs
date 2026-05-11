use crate::harness;

#[test]
#[ignore]
fn search_fields_name_logs() {
    if harness::require_creds("search_fields_name_logs").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "search-fields",
        "--name",
        "http response",
        "--dataset",
        "logs",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(
        &v,
        &["dataprime_path", "description", "similarity"],
    );
}

#[test]
#[ignore]
fn search_fields_name_spans() {
    if harness::require_creds("search_fields_name_spans").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "search-fields",
        "--name",
        "trace id",
        "--dataset",
        "spans",
        "--limit",
        "3",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(
        &v,
        &["dataprime_path", "description", "similarity"],
    );
}

#[test]
#[ignore]
fn search_fields_value_logs() {
    if harness::require_creds("search_fields_value_logs").is_none() {
        return;
    }
    // POST /api/v1/search-by-value — may return empty for accounts without indexed values
    let output = harness::cx()
        .args([
            "search-fields",
            "--value",
            "error",
            "--dataset",
            "logs",
            "--limit",
            "5",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to execute cx");
    assert!(
        output.status.success(),
        "search-fields --value should exit 0 even with empty results"
    );
}
