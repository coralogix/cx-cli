use crate::harness;

#[test]
#[ignore]
fn extensions_list() {
    if harness::require_creds("extensions_list").is_none() {
        return;
    }
    // The public catalog can be empty. Hidden extensions stay out of this list.
    let v = harness::run_ok_json(&["integrations", "extensions", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn extensions_deployed() {
    if harness::require_creds("extensions_deployed").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["integrations", "extensions", "deployed", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "version", "applications"]);
    // Guard against schema drift: key presence alone is not enough (the
    // pre-fix output had all keys present but null-valued).
    for item in v.as_array().unwrap() {
        assert!(
            item["id"].is_string(),
            "deployed extension id should be a non-null string: {item}"
        );
        assert!(
            item["version"].is_string(),
            "deployed extension version should be a non-null string: {item}"
        );
    }
}
