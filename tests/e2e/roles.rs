use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn roles_list() {
    if harness::require_creds("roles_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["roles", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["role_id", "name"]);
}

#[test]
#[ignore]
fn roles_get() {
    if harness::require_creds("roles_get").is_none() {
        return;
    }
    let id = match discover_role_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping roles_get: no roles available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["roles", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["role_id", "name"]);
}

#[test]
#[ignore]
fn roles_system() {
    if harness::require_creds("roles_system").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["roles", "system", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["role_id", "name"]);
}

fn discover_role_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("roles_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["roles", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            // Try flat array with "role_id"
            if let Some(arr) = v.as_array() {
                if let Some(id) = arr
                    .first()
                    .and_then(|item| item.get("role_id"))
                    .map(|x| x.to_string().trim_matches('"').to_string())
                {
                    return Some(id);
                }
            }
            // Try wrapped "roles" array with "roleId"
            v.get("roles")
                .and_then(|r| r.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| {
                    item.get("roleId")
                        .or_else(|| item.get("role_id"))
                        .map(|x| x.to_string().trim_matches('"').to_string())
                })
        })
        .clone()
}
