use crate::harness;

#[test]
#[ignore]
fn retentions_list() {
    if harness::require_creds("retentions_list").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["retentions", "list", "-o", "json"]);
}

#[test]
#[ignore]
fn retentions_status() {
    if harness::require_creds("retentions_status").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["retentions", "status", "-o", "json"]);
}
