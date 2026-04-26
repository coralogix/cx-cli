use crate::harness;

#[test]
#[ignore]
fn search_fields_logs() {
    if harness::require_creds("search_fields_logs").is_none() {
        return;
    }
    harness::run_ok_json(&[
        "search-fields",
        "http response",
        "--dataset",
        "logs",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
}
