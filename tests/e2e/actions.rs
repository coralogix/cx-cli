use crate::harness;

#[test]
#[ignore]
fn actions_list() {
    if harness::require_creds("actions_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["actions", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}
