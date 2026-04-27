use crate::harness;

#[test]
#[ignore]
fn presets_list() {
    if harness::require_creds("presets_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["presets", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}
