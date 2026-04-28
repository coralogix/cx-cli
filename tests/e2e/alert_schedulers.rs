use crate::harness;

#[test]
#[ignore]
fn alert_schedulers_list() {
    if harness::require_creds("alert_schedulers_list").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["alert-schedulers", "list", "-o", "json"]);
}
