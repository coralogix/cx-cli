use crate::harness;

#[test]
#[ignore]
fn scopes_list() {
    if harness::require_creds("scopes_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["scopes", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}
