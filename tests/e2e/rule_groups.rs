use crate::harness;

#[test]
#[ignore]
fn rule_groups_list() {
    if harness::require_creds("rule_groups_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["rule-groups", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn rule_groups_usage_limits() {
    if harness::require_creds("rule_groups_usage_limits").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["rule-groups", "usage-limits", "-o", "json"]);
}
