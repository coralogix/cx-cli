use crate::harness;

#[test]
#[ignore]
fn dashboards_catalog() {
    if harness::require_creds("dashboards_catalog").is_none() {
        return;
    }
    harness::run_ok_json(&["dashboards", "catalog", "-o", "json"]);
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
    harness::run_ok_json(&["dashboards", "get", &id, "-o", "json"]);
}
