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
fn search_fields_value_logs() {
    if harness::require_creds("search_fields_value_logs").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "search-fields",
        "--value",
        "log",
        "--dataset",
        "logs",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    let arr = v.as_array().expect("should be a JSON array");
    assert!(
        !arr.is_empty(),
        "search-fields --value 'log' should return at least one result"
    );
    harness::assert_array_of_objects_with_keys(&v, &["dataprime_path", "value", "similarity"]);
}
