use crate::harness;

#[test]
#[ignore]
fn ip_access_get() {
    if harness::require_creds("ip_access_get").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["ip-access", "get", "-o", "json"]);
}
