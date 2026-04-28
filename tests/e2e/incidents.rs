use crate::harness;

#[test]
#[ignore]
fn incidents_list() {
    if harness::require_creds("incidents_list").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["incidents", "list", "-o", "json"]);
}
