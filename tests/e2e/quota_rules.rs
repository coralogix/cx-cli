use crate::harness;

#[test]
#[ignore]
fn quota_rules_get() {
    if harness::require_creds("quota_rules_get").is_none() {
        return;
    }
    // The quota-rules endpoint requires elevated permissions that the test
    // API key may not have. Skip gracefully on auth errors.
    let _v = harness::run_tolerant_json(&["quotas", "get", "-o", "json"], "quota_rules_get");
}
