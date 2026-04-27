use crate::harness;

#[test]
#[ignore]
fn api_keys_list() {
    if harness::require_creds("api_keys_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["api-keys", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn api_keys_send_data_keys() {
    if harness::require_creds("api_keys_send_data_keys").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["api-keys", "send-data-keys", "-o", "json"]);
}
