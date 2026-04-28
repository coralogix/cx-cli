use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn connectors_list() {
    if harness::require_creds("connectors_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["notifications", "connectors", "list", "-o", "json"]);
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
    let v = harness::run_ok_json(&["notifications", "connectors", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn connectors_types() {
    if harness::require_creds("connectors_types").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["notifications", "connectors", "types", "-o", "json"]);
}

fn discover_connector_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("connectors_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["notifications", "connectors", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string().trim_matches('"').to_string(),
                })
        })
        .clone()
}
