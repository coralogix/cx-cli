use crate::harness;

#[test]
#[ignore]
fn integrations_list() {
    if harness::require_creds("integrations_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["integrations", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}
