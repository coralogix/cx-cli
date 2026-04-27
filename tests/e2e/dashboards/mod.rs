use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn dashboards_catalog() {
    if harness::require_creds("dashboards_catalog").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["dashboards", "catalog", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn dashboards_get() {
    if harness::require_creds("dashboards_get").is_none() {
        return;
    }
    let Some(id) = discover_dashboard_id() else {
        eprintln!("[e2e] skipping dashboards_get: no dashboards available on test team");
        return;
    };
    let v = harness::run_ok_json(&["dashboards", "get", &id, "-o", "json"]);
    // Shape varies (top-level vs nested under "dashboard") — only assert object.
    harness::assert_object_with_keys(&v, &[]);
}

/// Discover a dashboard id from `dashboards catalog -o json`.
fn discover_dashboard_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["dashboards", "catalog", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .filter_map(|item| item.get("id").and_then(|x| x.as_str()))
                .next()
                .map(String::from)
        })
        .clone()
}
