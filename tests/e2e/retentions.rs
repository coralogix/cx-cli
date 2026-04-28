use crate::harness;

#[test]
#[ignore]
fn retentions_list() {
    if harness::require_creds("retentions_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["retentions", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn retentions_status() {
    if harness::require_creds("retentions_status").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["retentions", "status", "-o", "json"]);
}
