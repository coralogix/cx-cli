use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn scopes_list() {
    if harness::require_creds("scopes_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["scopes", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn scopes_get() {
    if harness::require_creds("scopes_get").is_none() {
        return;
    }
    let id = match discover_scope_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping scopes_get: no scopes available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["scopes", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["id"]);
}

fn discover_scope_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("scopes_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["scopes", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .and_then(|x| x.as_str())
                .map(String::from)
        })
        .clone()
}
