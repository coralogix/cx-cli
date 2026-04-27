use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn slos_list() {
    if harness::require_creds("slos_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["slos", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name", "target"]);
}

#[test]
#[ignore]
fn slos_get() {
    if harness::require_creds("slos_get").is_none() {
        return;
    }
    let Some(id) = discover_slo_id() else {
        eprintln!("[e2e] skipping slos_get: no SLOs available on test team");
        return;
    };
    let v = harness::run_ok_json(&["slos", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["slo"]);
}

fn discover_slo_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["slos", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .filter_map(|item| item.get("id").and_then(|x| x.as_str()))
                .next()
                .map(String::from)
        })
        .clone()
}
