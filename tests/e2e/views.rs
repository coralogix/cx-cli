use crate::harness;

#[test]
#[ignore]
fn views_list() {
    if harness::require_creds("views_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["views", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn views_folders_list() {
    if harness::require_creds("views_folders_list").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["views", "folders", "list", "-o", "json"]);
}
