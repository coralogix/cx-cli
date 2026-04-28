use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn tco_policies_list() {
    if harness::require_creds("tco_policies_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["tco-policies", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn tco_policies_get() {
    if harness::require_creds("tco_policies_get").is_none() {
        return;
    }
    let id = match discover_tco_policy_id() {
        Some(id) => id,
        None => {
            eprintln!(
                "[e2e] skipping tco_policies_get: no TCO policies available on test team"
            );
            return;
        }
    };
    let v = harness::run_ok_json(&["tco-policies", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn tco_policies_settings() {
    if harness::require_creds("tco_policies_settings").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["tco-policies", "settings", "-o", "json"]);
}

fn discover_tco_policy_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("tco_policies_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["tco-policies", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .and_then(|x| x.as_str())
                .map(String::from)
        })
        .clone()
}
