use crate::harness;

#[test]
#[ignore]
fn incidents_list() {
    if harness::require_creds("incidents_list").is_none() {
        return;
    }
    // The incidents endpoint may return 504 gateway timeouts under load.
    // Use run_tolerant_json to skip gracefully instead of failing the suite.
    let _v = harness::run_tolerant_json(&["incidents", "list", "-o", "json"], "incidents_list");
}
