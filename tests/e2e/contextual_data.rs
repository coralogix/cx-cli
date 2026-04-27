use crate::harness;

#[test]
#[ignore]
fn contextual_data_list() {
    if harness::require_creds("contextual_data_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["contextual-data", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}
