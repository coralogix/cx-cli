use crate::harness;

#[test]
#[ignore]
fn roles_list() {
    if harness::require_creds("roles_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["roles", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["role_id", "name"]);
}

#[test]
#[ignore]
fn roles_system() {
    if harness::require_creds("roles_system").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["roles", "system", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["role_id", "name"]);
}
