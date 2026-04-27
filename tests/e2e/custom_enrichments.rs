use crate::harness;

#[test]
#[ignore]
fn custom_enrichments_list() {
    if harness::require_creds("custom_enrichments_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["custom-enrichments", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}
