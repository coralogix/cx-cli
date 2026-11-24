use serde_json::{json, Value};

use crate::harness;

const FALLBACK_APP: &str = "cx-e2e-app";

fn product_analytics_available(test_name: &str) -> bool {
    let output = harness::run_raw(&["product-analytics", "funnels", "list", "-o", "json"]);
    if output.status.success() {
        return true;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("(412)") || stderr.contains("(501)") {
        eprintln!(
            "[e2e] not covered: {test_name} — Product Analytics API not deployed or not enabled for the team"
        );
        return false;
    }
    panic!("product-analytics funnels list failed unexpectedly:\n{stderr}");
}

fn discover_app() -> Option<String> {
    let apps = harness::run_ok_json(&[
        "product-analytics",
        "applications",
        "list",
        "--start",
        "now-7d",
        "-o",
        "json",
    ]);
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
    let views = harness::run_ok_json(&[
        "product-analytics",
        "views",
        "list",
        "--app",
        &app,
        "--start",
        "now-7d",
        "-o",
        "json",
    ]);
    for args in [
        vec!["actions", "list", "--app", &app],
        vec!["actions", "list", "--app", &app, "--type", "custom"],
        vec!["filter-values", "--app", &app, "--field", "platform"],
    ] {
        let mut command = vec!["product-analytics"];
        command.extend(args);
        command.extend(["--start", "now-7d", "-o", "json"]);
        harness::assert_array(&harness::run_ok_json(&command));
    }
    let Some(view) = harness::assert_array(&views)
        .first()
        .and_then(|v| v["viewName"].as_str())
        .map(str::to_string)
    else {
        return;
    };
    for kind in [["pathways", "query"], ["heatmaps", "query"]] {
        harness::run_ok_json(&[
            "product-analytics",
            kind[0],
            kind[1],
            "--app",
            &app,
            "--view",
            &view,
            "--start",
            "now-7d",
            "-o",
            "json",
        ]);
    }
    harness::run_ok_json(&[
        "product-analytics",
        "heatmaps",
        "scroll-map",
        "--app",
        &app,
        "--view",
        &view,
        "--start",
        "now-7d",
        "-o",
        "json",
    ]);
}

fn funnel_body(app: &str, name: &str) -> Value {
    json!({
        "appNames": [app],
        "name": name,
        "steps": [
            {"stepId": "s1", "conditions": [{"page": {"match": {"values": ["/"], "operator": "EQUAL"}}}]},
            {"stepId": "s2", "conditions": [{"page": {"match": {"values": ["explore"], "operator": "EQUAL"}}}]}
        ],
        "timeRange": {"relativeMinutes": "1440"}
    })
}

fn pathway_body(app: &str, name: &str) -> Value {
    json!({
        "appName": app,
        "name": name,
        "pageName": "/",
        "pagesCount": 5,
        "limit": 20,
        "timeRange": {"relativeMinutes": "1440"}
    })
}

fn heatmap_body(app: &str, name: &str) -> Value {
    json!({
        "appName": app,
        "name": name,
        "viewName": "/",
        "mode": "TOP_ELEMENTS",
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

    if kind == "funnels" {
        harness::run_ok_json(&["product-analytics", "funnels", "query", &id, "-o", "json"]);
        harness::run_ok_json(&[
            "product-analytics",
            "funnels",
            "matching-users",
            &id,
            "--outcome",
            "converted",
            "--limit",
            "5",
            "-o",
            "json",
        ]);
    }

    let got = harness::run_ok_json(&["product-analytics", kind, "get", &id, "-o", "json"]);
    assert_eq!(got["name"], name);
    let list = harness::run_ok_json(&[
        "product-analytics",
        kind,
        "list",
        "--app",
        &app,
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
    harness::run_write_json(&[
        "product-analytics",
        kind,
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let updated = harness::run_ok_json(&["product-analytics", kind, "get", &id, "-o", "json"]);
    assert_eq!(updated["name"], format!("{name}-2"));
    let mut kept = updated.clone();
    let mut original = got.clone();
    for value in [&mut kept, &mut original] {
        let map = value
            .as_object_mut()
            .unwrap_or_else(|| panic!("{kind} get returned a non-object"));
        for field in ["name", "id", "appName", "createdAt", "updatedAt"] {
            map.remove(field);
        }
    }
    assert_eq!(
        kept, original,
        "{kind} update must keep every field it was sent"
    );
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
