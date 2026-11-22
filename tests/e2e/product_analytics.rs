use serde_json::{json, Value};

use crate::harness;

const FALLBACK_APP: &str = "cx-e2e-app";

fn product_analytics_available(test_name: &str) -> bool {
    let output = harness::run_raw(&["product-analytics", "funnels", "list", "-o", "json"]);
    if output.status.success() {
        return true;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("(412)") || stderr.contains("(404)") || stderr.contains("(501)") {
        eprintln!(
            "[e2e] skipping {test_name}: Product Analytics API not deployed or not enabled for the team"
        );
        return false;
    }
    panic!("product-analytics funnels list failed unexpectedly:\n{stderr}");
}

fn discover_app() -> Option<String> {
    let apps = harness::run_ok_json(&["product-analytics", "applications", "list", "-o", "json"]);
    harness::assert_array(&apps)
        .first()
        .and_then(|app| app["appName"].as_str())
        .map(str::to_string)
}

#[test]
#[ignore]
fn product_analytics_settings_get() {
    if harness::require_creds("product_analytics_settings_get").is_none() {
        return;
    }
    if !product_analytics_available("product_analytics_settings_get") {
        return;
    }
    harness::run_ok_json(&["product-analytics", "settings", "get", "-o", "json"]);
    harness::run_ok_nonempty(&["product-analytics", "settings", "get"]);
}

#[test]
#[ignore]
fn product_analytics_settings_roundtrip() {
    if harness::require_write_creds("product_analytics_settings_roundtrip").is_none() {
        return;
    }
    if !product_analytics_available("product_analytics_settings_roundtrip") {
        return;
    }
    let before = harness::run_ok_json(&["product-analytics", "settings", "get", "-o", "json"]);
    let enabled = before["enabled"].as_bool().unwrap_or(false).to_string();
    harness::run_write_ok(&[
        "product-analytics",
        "settings",
        "update",
        "--enabled",
        &enabled,
    ]);
    let after = harness::run_ok_json(&["product-analytics", "settings", "get", "-o", "json"]);
    assert_eq!(
        after["enabled"].as_bool().unwrap_or(false).to_string(),
        enabled
    );
}

#[test]
#[ignore]
fn product_analytics_read_commands() {
    if harness::require_creds("product_analytics_read_commands").is_none() {
        return;
    }
    if !product_analytics_available("product_analytics_read_commands") {
        return;
    }
    for kind in ["funnels", "pathways", "heatmaps"] {
        let list = harness::run_ok_json(&["product-analytics", kind, "list", "-o", "json"]);
        harness::assert_array_of_objects_with_keys(&list, &["id", "name", "appName"]);
    }
    let Some(app) = discover_app() else {
        eprintln!("[e2e] product_analytics_read_commands: no RUM application with sessions");
        return;
    };
    harness::run_ok_json(&[
        "product-analytics",
        "applications",
        "get",
        &app,
        "-o",
        "json",
    ]);
    let views = harness::run_ok_json(&[
        "product-analytics",
        "views",
        "list",
        "--app",
        &app,
        "--last",
        "7d",
        "-o",
        "json",
    ]);
    let Some(view) = harness::assert_array(&views)
        .first()
        .and_then(|v| v["view"].as_str())
        .map(str::to_string)
    else {
        return;
    };
    let events = harness::run_ok_json(&[
        "product-analytics",
        "events",
        "list",
        "--app",
        &app,
        "--view",
        &view,
        "--source",
        "auto",
        "--last",
        "7d",
        "-o",
        "json",
    ]);
    if let Some(event_id) = harness::assert_array(&events)
        .first()
        .and_then(|e| e["eventId"].as_str())
    {
        harness::run_ok_json(&[
            "product-analytics",
            "events",
            "get",
            event_id,
            "--app",
            &app,
            "--view",
            &view,
            "--last",
            "7d",
            "-o",
            "json",
        ]);
    }
    harness::run_ok_json(&[
        "product-analytics",
        "pathways",
        "query",
        "--app",
        &app,
        "--view",
        &view,
        "--last",
        "7d",
        "-o",
        "json",
    ]);
    harness::run_ok_json(&[
        "product-analytics",
        "heatmaps",
        "query",
        "--app",
        &app,
        "--view",
        &view,
        "--last",
        "7d",
        "-o",
        "json",
    ]);
}

fn funnel_body(app: &str, name: &str) -> Value {
    json!({
        "appName": app,
        "name": name,
        "definition": {
            "mode": "FUNNEL_MODE_USERS",
            "steps": [
                {"stepId": "s1", "conditions": [{"view": {}}]},
                {"stepId": "s2", "conditions": [{"action": {}}]}
            ]
        },
        "timeRange": {"relativeMinutes": "1440"}
    })
}

fn pathway_body(app: &str, name: &str) -> Value {
    json!({
        "appName": app,
        "name": name,
        "view": "/",
        "timeRange": {"relativeMinutes": "1440"}
    })
}

fn heatmap_body(app: &str, name: &str) -> Value {
    json!({
        "appName": app,
        "name": name,
        "viewName": "/",
        "mode": "HEATMAP_MODE_TOP_ELEMENTS",
        "timeRange": {"calendar": "CALENDAR_WINDOW_TODAY"}
    })
}

fn saved_lifecycle(kind: &str, body: fn(&str, &str) -> Value) {
    let app = discover_app().unwrap_or_else(|| FALLBACK_APP.to_string());
    let name = harness::unique_name(kind);
    let file = harness::JsonFile::new(kind, &body(&app, &name));
    let created = harness::run_write_json(&[
        "product-analytics",
        kind,
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("{kind} create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["product-analytics", kind, "delete", &id]);
    assert_eq!(created["name"], name);
    assert_eq!(created["version"], 1);

    let got = harness::run_ok_json(&["product-analytics", kind, "get", &id, "-o", "json"]);
    assert_eq!(got["name"], name);
    let list = harness::run_ok_json(&[
        "product-analytics",
        kind,
        "list",
        "--app",
        &app,
        "--page-size",
        "500",
        "-o",
        "json",
    ]);
    assert!(
        harness::find_by_name(&list, "name", &name).is_some(),
        "{kind} list should contain {name}"
    );

    let mut edited = got.clone();
    edited["name"] = json!(format!("{name}-2"));
    let upd = harness::JsonFile::new(&format!("{kind}-upd"), &edited);
    let updated = harness::run_write_json(&[
        "product-analytics",
        kind,
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    assert_eq!(updated["name"], format!("{name}-2"));
    assert_eq!(updated["version"], 2);
    harness::run_ok_nonempty(&["product-analytics", kind, "get", &id]);
    harness::run_ok_nonempty(&["product-analytics", kind, "list", "-o", "toon"]);

    harness::run_write_ok(&["product-analytics", kind, "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["product-analytics", kind, "get", &id, "-o", "json"]);
}

#[test]
#[ignore]
fn product_analytics_funnels_lifecycle() {
    if harness::require_write_creds("product_analytics_funnels_lifecycle").is_none() {
        return;
    }
    if !product_analytics_available("product_analytics_funnels_lifecycle") {
        return;
    }
    saved_lifecycle("funnels", funnel_body);
}

#[test]
#[ignore]
fn product_analytics_pathways_lifecycle() {
    if harness::require_write_creds("product_analytics_pathways_lifecycle").is_none() {
        return;
    }
    if !product_analytics_available("product_analytics_pathways_lifecycle") {
        return;
    }
    saved_lifecycle("pathways", pathway_body);
}

#[test]
#[ignore]
fn product_analytics_heatmaps_lifecycle() {
    if harness::require_write_creds("product_analytics_heatmaps_lifecycle").is_none() {
        return;
    }
    if !product_analytics_available("product_analytics_heatmaps_lifecycle") {
        return;
    }
    saved_lifecycle("heatmaps", heatmap_body);
}
