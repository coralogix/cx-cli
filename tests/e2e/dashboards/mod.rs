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
    let Some(id) = harness::discover_dashboard_id() else {
        eprintln!("[e2e] skipping dashboards_get: no dashboards available in staging");
        return;
    };
    let v = harness::run_ok_json(&["dashboards", "get", &id, "-o", "json"]);
    // Shape varies (top-level vs nested under "dashboard") — only assert object.
    harness::assert_object_with_keys(&v, &[]);
}
