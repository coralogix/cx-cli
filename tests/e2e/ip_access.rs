use crate::harness;

#[test]
#[ignore]
fn ip_access_get() {
    if harness::require_creds("ip_access_get").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["iam", "ip-access", "get", "-o", "json"]);
    harness::assert_get_response(&v, &["id", "ipAccess"]);
    harness::run_ok_nonempty(&["iam", "ip-access", "get"]);
    harness::run_ok_nonempty(&["iam", "ip-access", "get", "-o", "toon"]);
}
