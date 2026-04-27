use crate::harness;

#[test]
#[ignore]
fn search_fields_logs() {
    if harness::require_creds("search_fields_logs").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "search-fields",
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
