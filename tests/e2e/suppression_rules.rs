use crate::harness;

#[test]
#[ignore]
fn suppression_rules_list() {
    if harness::require_creds("suppression_rules_list").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["alerts", "suppression-rules", "list", "-o", "json"]);
}
