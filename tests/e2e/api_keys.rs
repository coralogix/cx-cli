use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn api_keys_list() {
    if harness::require_creds("api_keys_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["api-keys", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn api_keys_get() {
    if harness::require_creds("api_keys_get").is_none() {
        return;
    }
    let id = match discover_api_key_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping api_keys_get: no API keys available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["api-keys", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn api_keys_send_data_keys() {
    if harness::require_creds("api_keys_send_data_keys").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["api-keys", "send-data-keys", "-o", "json"]);
}

fn discover_api_key_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("api_keys_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["api-keys", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            // The list response may be a flat array with "id" field,
            // or wrapped in a "keys" array with nested "keyInfo.keyId".
            if let Some(arr) = v.as_array() {
                if let Some(id) = arr
                    .first()
                    .and_then(|item| item.get("id"))
                    .and_then(|x| x.as_str())
                {
                    return Some(id.to_string());
                }
                // Try nested keyInfo.keyId pattern
                if let Some(id) = arr
                    .first()
                    .and_then(|item| item.get("keyInfo"))
                    .and_then(|ki| ki.get("keyId"))
                    .and_then(|x| x.as_str())
                {
                    return Some(id.to_string());
                }
            }
            // Try wrapped "keys" array
            v.get("keys")
                .and_then(|k| k.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| {
                    item.get("keyInfo")
                        .and_then(|ki| ki.get("keyId"))
                        .and_then(|x| x.as_str())
                        .or_else(|| item.get("id").and_then(|x| x.as_str()))
                })
                .map(String::from)
        })
        .clone()
}
