use crate::harness;

#[test]
#[ignore]
fn routers_list() {
    if harness::require_creds("routers_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["routers", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}
