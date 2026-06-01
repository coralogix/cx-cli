use crate::harness;

#[test]
#[ignore]
fn incidents_list() {
    if harness::require_creds("incidents_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["incidents", "list", "--limit", "10", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(
        &v,
        &["id", "name", "severity", "state", "created_at"],
    );
}

#[test]
#[ignore]
fn incidents_list_with_filters() {
    if harness::require_creds("incidents_list_with_filters").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "incidents",
        "list",
        "--status",
        "TRIGGERED",
        "--severity",
        "CRITICAL",
        "--page-size",
        "1",
        "--limit",
        "1",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(
        &v,
        &["id", "name", "severity", "state", "created_at"],
    );
}
