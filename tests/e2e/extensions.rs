use crate::harness;

#[test]
#[ignore]
fn extensions_list() {
    if harness::require_creds("extensions_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["integrations", "extensions", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn extensions_deployed() {
    if harness::require_creds("extensions_deployed").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["integrations", "extensions", "deployed", "-o", "json"]);
}
