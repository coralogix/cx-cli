use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn integrations_list() {
    if harness::require_creds("integrations_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["integrations", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn integrations_get() {
    if harness::require_creds("integrations_get").is_none() {
        return;
    }
    let id = match discover_integration_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping integrations_get: no integrations available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["integrations", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["id"]);
}

fn discover_integration_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("integrations_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["integrations", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .and_then(|x| x.as_str())
                .map(String::from)
        })
        .clone()
}
