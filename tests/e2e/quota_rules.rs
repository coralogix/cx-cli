use crate::harness;

#[test]
#[ignore]
fn quota_rules_get() {
    if harness::require_creds("quota_rules_get").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["quota-rules", "get", "-o", "json"]);
}
