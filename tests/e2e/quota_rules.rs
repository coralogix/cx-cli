use crate::harness;

#[test]
#[ignore]
fn quota_rules_get() {
    if harness::require_creds("quota_rules_get").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["quotas", "get", "-o", "json"]);
}
