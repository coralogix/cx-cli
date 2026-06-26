use crate::harness;

#[test]
#[ignore]
fn e2m_list() {
    if harness::require_creds("e2m_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["e2m", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn e2m_limits() {
    if harness::require_creds("e2m_limits").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["e2m", "limits", "-o", "json"]);
}
