//! Integration tests for dashboard management commands:
//! catalog, get, create, folders list, folders create.

mod common;

use std::path::PathBuf;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::dashboards;
use coralogix_cli::config::OutputFormat;

fn temp_dashboard_json_path(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "cx_test_dashboard_{label}_{}.json",
        std::process::id()
    ));
    p
}

// ── catalog ───────────────────────────────────────────────────────────────────

// NOTE: basic happy-path coverage for run_catalog is already in
// tests/dashboards/main.rs::run_catalog_json_output_succeeds.
// Tests here cover the cases not present there.

#[tokio::test]
async fn dashboard_catalog_empty_returns_ok() {
    let server = MockServer::start().await;

    let body = json!({ "items": [] });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/dashboards/dashboards/v1/catalog/list",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_catalog(&targets, OutputFormat::Json)
        .await
        .expect("empty catalog should succeed");
}

#[tokio::test]
async fn dashboard_catalog_500_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/dashboards/dashboards/v1/catalog/list",
        ))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result = dashboards::run_catalog(&targets, OutputFormat::Json).await;
    assert!(result.is_err(), "500 should return Err for catalog");
}

#[tokio::test]
async fn dashboard_catalog_multi_profile_merges() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    let body_a = json!({
        "items": [{"id": "d-a", "name": "DashA", "description": null, "slugName": null,
                   "createTime": null, "updateTime": null, "isDefault": null,
                   "isPinned": null, "isLocked": null, "folder": null}]
    });
    let body_b = json!({
        "items": [{"id": "d-b", "name": "DashB", "description": null, "slugName": null,
                   "createTime": null, "updateTime": null, "isDefault": null,
                   "isPinned": null, "isLocked": null, "folder": null}]
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/dashboards/dashboards/v1/catalog/list",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body_a))
        .mount(&server_a)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/dashboards/dashboards/v1/catalog/list",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body_b))
        .mount(&server_b)
        .await;

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    dashboards::run_catalog(&targets, OutputFormat::Json)
        .await
        .expect("multi-profile catalog should succeed");
}

#[tokio::test]
async fn dashboard_catalog_all_profiles_fail_returns_error() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/dashboards/dashboards/v1/catalog/list",
        ))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server_a)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/dashboards/dashboards/v1/catalog/list",
        ))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server_b)
        .await;

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    let result = dashboards::run_catalog(&targets, OutputFormat::Json).await;
    assert!(
        result.is_err(),
        "when every profile fails, run_catalog should return Err for CI/scripts"
    );
}

#[tokio::test]
async fn dashboard_catalog_agents_output_succeeds() {
    let server = MockServer::start().await;
    let body = json!({ "items": [] });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/dashboards/dashboards/v1/catalog/list",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    dashboards::run_catalog(&[target], OutputFormat::Agents)
        .await
        .expect("run_catalog with agents (TOON) should succeed");
}

// ── get ───────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dashboard_get_returns_result() {
    let server = MockServer::start().await;

    let body = json!({
        "id": "dash-001",
        "name": "API Overview",
        "layout": {"sections": []}
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1/dash-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_get(&targets, "dash-001", OutputFormat::Json)
        .await
        .expect("get should succeed");
}

#[tokio::test]
async fn dashboard_get_404_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1/nonexistent"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({"message": "Dashboard not found"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result = dashboards::run_get(&targets, "nonexistent", OutputFormat::Json).await;
    assert!(result.is_err(), "404 should return Err for get");
}

#[tokio::test]
async fn dashboard_get_empty_id_returns_error() {
    let target = common::test_target("test-profile", "http://127.0.0.1:1");
    let targets = vec![target];
    let result = dashboards::run_get(&targets, "   ", OutputFormat::Json).await;
    assert!(result.is_err(), "blank dashboard id should return Err");
}

#[tokio::test]
async fn dashboard_get_multi_profile_includes_profile_field() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    let body = json!({"id": "dash-001", "name": "Shared", "layout": {}});

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1/dash-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server_a)
        .await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1/dash-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server_b)
        .await;

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    dashboards::run_get(&targets, "dash-001", OutputFormat::Json)
        .await
        .expect("multi-profile get should succeed");
}

// ── create ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dashboard_create_from_stdin_succeeds() {
    // We cannot easily pipe stdin in unit tests; test the file path instead.
    // This test exercises the API call path via a temp file.
    let server = MockServer::start().await;

    let response = json!({ "dashboardId": "new-dash-abc" });

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .expect(1)
        .mount(&server)
        .await;

    let file_path = temp_dashboard_json_path("create_ok");
    std::fs::write(
        &file_path,
        r#"{"name": "Test Dashboard", "layout": {"sections": []}}"#,
    )
    .expect("write temp file");

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_create(
        &targets,
        file_path.to_str().unwrap(),
        None,
        OutputFormat::Json,
    )
    .await
    .expect("create should succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("server should record requests");
    assert_eq!(reqs.len(), 1);
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    assert!(
        body.get("requestId").is_some(),
        "requestId must be injected by run_create"
    );
    assert_eq!(
        body["dashboard"]["name"], "Test Dashboard",
        "dashboard body must be wrapped under 'dashboard' key"
    );

    let _ = std::fs::remove_file(&file_path);
}

#[tokio::test]
async fn dashboard_create_api_500_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let file_path = temp_dashboard_json_path("create_500");
    std::fs::write(
        &file_path,
        r#"{"name": "Test", "layout": {"sections": []}}"#,
    )
    .expect("write temp file");

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result = dashboards::run_create(
        &targets,
        file_path.to_str().unwrap(),
        None,
        OutputFormat::Json,
    )
    .await;
    assert!(result.is_err(), "500 should return Err for create");
    let _ = std::fs::remove_file(&file_path);
}

#[tokio::test]
async fn dashboard_create_missing_layout_returns_error_before_api_call() {
    // No mock needed — the validation error must fire before any HTTP request.
    let file_path = temp_dashboard_json_path("create_bad_layout");
    std::fs::write(&file_path, r#"{"name": "Missing Layout"}"#).expect("write temp file");

    // Use a port that is not listening to ensure no request is made.
    let target = common::test_target("test-profile", "http://127.0.0.1:1");
    let targets = vec![target];

    let result = dashboards::run_create(
        &targets,
        file_path.to_str().unwrap(),
        None,
        OutputFormat::Json,
    )
    .await;
    assert!(result.is_err(), "missing 'layout' should return Err");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("layout"),
        "error should mention missing 'layout' field, got: {msg}"
    );
    let _ = std::fs::remove_file(&file_path);
}

// ── folders list ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn dashboard_folders_list_returns_results() {
    let server = MockServer::start().await;

    let body = json!({
        "folder": [
            {"id": "f-001", "name": "Engineering", "parentId": null},
            {"id": "f-002", "name": "Backend", "parentId": "f-001"}
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dashboards/folders/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_folders_list(&targets, OutputFormat::Json)
        .await
        .expect("folders list should succeed");
}

#[tokio::test]
async fn dashboard_folders_list_empty_returns_ok() {
    let server = MockServer::start().await;

    let body = json!({ "folder": [] });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dashboards/folders/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_folders_list(&targets, OutputFormat::Json)
        .await
        .expect("empty folders list should succeed");
}

#[tokio::test]
async fn dashboard_folders_list_500_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dashboards/folders/v1"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result = dashboards::run_folders_list(&targets, OutputFormat::Json).await;
    assert!(result.is_err(), "500 should return Err for folders list");
}

#[tokio::test]
async fn dashboard_folders_list_multi_profile_merges() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    let body_a = json!({ "folder": [{"id": "f-a", "name": "FolderA", "parentId": null}] });
    let body_b = json!({ "folder": [{"id": "f-b", "name": "FolderB", "parentId": null}] });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dashboards/folders/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body_a))
        .mount(&server_a)
        .await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dashboards/folders/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body_b))
        .mount(&server_b)
        .await;

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    dashboards::run_folders_list(&targets, OutputFormat::Json)
        .await
        .expect("multi-profile folders list should succeed");
}

// ── folders create ────────────────────────────────────────────────────────────

#[tokio::test]
async fn dashboard_folders_create_succeeds() {
    let server = MockServer::start().await;

    let response = json!({ "folderId": "new-folder-xyz" });

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/folders/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_folders_create(&targets, "My Folder", None, OutputFormat::Json)
        .await
        .expect("folders create should succeed");
}

#[tokio::test]
async fn dashboard_folders_create_with_parent_id_succeeds() {
    let server = MockServer::start().await;

    let response = json!({ "folderId": "child-folder-abc" });

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/folders/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_folders_create(
        &targets,
        "Child Folder",
        Some("parent-folder-id"),
        OutputFormat::Json,
    )
    .await
    .expect("folders create with parent should succeed");
}

#[tokio::test]
async fn dashboard_folders_create_500_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/folders/v1"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result =
        dashboards::run_folders_create(&targets, "Broken Folder", None, OutputFormat::Json).await;
    assert!(result.is_err(), "500 should return Err for folders create");
}

#[tokio::test]
async fn dashboard_folders_create_empty_name_returns_error() {
    let target = common::test_target("test-profile", "http://127.0.0.1:1");
    let targets = vec![target];
    let result = dashboards::run_folders_create(&targets, "  ", None, OutputFormat::Json).await;
    assert!(result.is_err(), "blank folder name should return Err");
}
