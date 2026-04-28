use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn slos_list() {
    if harness::require_creds("slos_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["slos", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name", "target"]);
}

#[test]
#[ignore]
fn slos_get() {
    if harness::require_creds("slos_get").is_none() {
        return;
    }
    let id = match discover_slo_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping slos_get: no SLOs available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["slos", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["slo"]);
}

fn discover_slo_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("slos_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["slos", "list", "-o", "json"]);
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
