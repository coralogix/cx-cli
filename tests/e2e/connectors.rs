use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn connectors_list() {
    if harness::require_creds("connectors_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["connectors", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn connectors_get() {
    if harness::require_creds("connectors_get").is_none() {
        return;
    }
    let id = match discover_connector_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping connectors_get: no connectors available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["connectors", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn connectors_types() {
    if harness::require_creds("connectors_types").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["connectors", "types", "-o", "json"]);
}

#[test]
#[ignore]
fn connectors_entity_types() {
    if harness::require_creds("connectors_entity_types").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["connectors", "entity-types", "-o", "json"]);
}

fn discover_connector_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("connectors_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["connectors", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .and_then(|x| x.as_str())
                .map(String::from)
        })
        .clone()
}
