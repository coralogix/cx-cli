use crate::harness;

#[test]
#[ignore]
fn users_search() {
    if harness::require_creds("users_search").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["users", "search", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["user_id", "name"]);
}
