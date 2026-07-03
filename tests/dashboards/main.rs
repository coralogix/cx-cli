#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::dashboards::api::DashboardsApi;
use coralogix_cli::commands::dashboards::{
    run_catalog, run_check, run_delete, run_folders_delete, run_replace,
};
use coralogix_cli::config::OutputFormat;

/// Verify that `DashboardsApi::catalog()` correctly deserializes a mocked
/// response from the dashboard catalog endpoint.
#[tokio::test]
async fn catalog_returns_items_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "items": [
            {
                "id": "dash-001",
                "name": "Production Overview",
                "description": "Main production dashboard",
                "slugName": "production-overview",
                "createTime": "2024-01-01T00:00:00Z",
                "updateTime": "2024-06-15T12:00:00Z",
                "isDefault": false,
                "isPinned": true,
                "isLocked": false,
                "folder": {
                    "id": "folder-01",
                    "name": "Operations",
                    "parentId": null
                }
            },
            {
                "id": "dash-002",
                "name": "Error Rates",
                "description": null,
                "slugName": "error-rates",
                "createTime": "2024-03-10T08:00:00Z",
                "updateTime": "2024-06-20T09:30:00Z",
                "isDefault": false,
                "isPinned": false,
                "isLocked": true,
                "folder": null
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/dashboards/dashboards/v1/catalog/list",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let api = DashboardsApi::new(&target.client);
    let response = api.catalog().await.expect("catalog() should succeed");

    assert_eq!(response.items.len(), 2);
    assert_eq!(response.items[0].id.as_deref(), Some("dash-001"));
    assert_eq!(
        response.items[0].name.as_deref(),
        Some("Production Overview")
    );
    assert_eq!(response.items[0].is_pinned, Some(true));
    assert_eq!(
        response.items[0]
            .folder
            .as_ref()
            .and_then(|f| f.name.as_deref()),
        Some("Operations")
    );
    assert_eq!(response.items[1].id.as_deref(), Some("dash-002"));
    assert_eq!(response.items[1].is_locked, Some(true));
    assert!(response.items[1].folder.is_none());
}

/// Verify that `run_catalog` (the full command runner) succeeds with JSON output
/// when backed by a mock server. This exercises the fan-out + rendering path.
#[tokio::test]
async fn run_catalog_json_output_succeeds() {
    let server = MockServer::start().await;

    let body = json!({
        "items": [
            {
                "id": "dash-100",
                "name": "Test Dashboard",
                "slugName": "test-dash",
                "isPinned": false,
                "isLocked": false
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/dashboards/dashboards/v1/catalog/list",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    run_catalog(&targets, OutputFormat::Json)
        .await
        .expect("run_catalog with JSON output should succeed");
}

/// Verify that `run_delete` succeeds when the mock returns an empty response.
#[tokio::test]
async fn delete_dashboard_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1/dash-abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    run_delete(&targets, "dash-abc")
        .await
        .expect("run_delete should succeed");
}

/// Verify that `DashboardsApi::replace()` sends a PUT and returns the response.
#[tokio::test]
async fn replace_dashboard_api() {
    let server = MockServer::start().await;

    let response_body = json!({
        "dashboard": {
            "id": "dash-001",
            "name": "Updated Dashboard"
        }
    });

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let api = DashboardsApi::new(&target.client);

    let body = json!({
        "requestId": "test-req-id",
        "dashboard": {
            "id": "dash-001",
            "name": "Updated Dashboard",
            "layout": {}
        }
    });

    let resp = api.replace(&body).await.expect("replace() should succeed");
    assert_eq!(
        resp.get("dashboard")
            .and_then(|d| d.get("id"))
            .and_then(|v| v.as_str()),
        Some("dash-001")
    );
}

/// Verify that `run_replace` succeeds with JSON output when backed by a mock server.
#[tokio::test]
async fn run_replace_json_output_succeeds() {
    let server = MockServer::start().await;

    let response_body = json!({
        "dashboard": {
            "id": "dash-001",
            "name": "Replaced Dashboard"
        }
    });

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    let tmp = std::env::temp_dir().join("cx_test_replace_dashboard.json");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&json!({
            "id": "dash-001",
            "name": "Replaced Dashboard",
            "layout": { "sections": [] }
        }))
        .unwrap(),
    )
    .unwrap();

    run_replace(
        &targets,
        tmp.to_str().unwrap(),
        OutputFormat::Json,
        true,
        false,
    )
    .await
    .expect("run_replace with JSON output should succeed");

    std::fs::remove_file(&tmp).ok();
}

/// Verify that `run_replace` succeeds with text output.
#[tokio::test]
async fn run_replace_text_output_succeeds() {
    let server = MockServer::start().await;

    let response_body = json!({
        "dashboardId": "dash-001"
    });

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    let tmp = std::env::temp_dir().join("cx_test_replace_dashboard_text.json");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&json!({
            "id": "dash-001",
            "name": "My Dashboard",
            "layout": { "sections": [] }
        }))
        .unwrap(),
    )
    .unwrap();

    run_replace(
        &targets,
        tmp.to_str().unwrap(),
        OutputFormat::Text,
        true,
        false,
    )
    .await
    .expect("run_replace with text output should succeed");

    std::fs::remove_file(&tmp).ok();
}

/// Verify that `run_replace` fails with a clear error when the dashboard JSON has no `id` field.
#[tokio::test]
async fn run_replace_missing_id_fails() {
    let server = MockServer::start().await;
    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    let tmp = std::env::temp_dir().join("cx_test_replace_no_id.json");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&serde_json::json!({
            "name": "Dashboard Without ID",
            "layout": { "sections": [] }
        }))
        .unwrap(),
    )
    .unwrap();

    let err = run_replace(
        &targets,
        tmp.to_str().unwrap(),
        OutputFormat::Text,
        true,
        false,
    )
    .await
    .expect_err("run_replace should fail when id is missing");

    assert!(
        err.to_string().contains("missing required 'id' field"),
        "error should mention missing id: {err}"
    );

    std::fs::remove_file(&tmp).ok();
}

/// Verify that `run_folders_delete` succeeds when the mock returns an empty response.
#[tokio::test]
async fn delete_dashboard_folder_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/mgmt/openapi/5/dashboards/folders/v1/folder-xyz"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    run_folders_delete(&targets, "folder-xyz")
        .await
        .expect("run_folders_delete should succeed");
}

// ── run_check (CheckDashboard) ────────────────────────────────────────────────

/// Path of the CheckDashboard endpoint, as called by `DashboardsApi::check`.
const CHECK_PATH: &str = "/mgmt/openapi/5/dashboards/check/v1";

/// Minimal valid dashboard JSON used for the `--from-file` check tests.
/// `read_dashboard_body` requires a `layout` field, so include one.
fn write_minimal_dashboard() -> std::path::PathBuf {
    let tmp = std::env::temp_dir().join("cx_test_check_dashboard.json");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&json!({
            "name": "Test Dashboard",
            "layout": { "sections": [] }
        }))
        .unwrap(),
    )
    .unwrap();
    tmp
}

/// Verify that `run_check --from-file` prints the green "valid" message and
/// exits 0 when the mock returns no issues.
#[tokio::test]
async fn run_check_from_file_text_output_valid() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(CHECK_PATH))
        .and(body_partial_json(
            json!({ "dashboard": { "layout": { "sections": [] } } }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "issues": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    let tmp = write_minimal_dashboard();

    run_check(
        &targets,
        Some(tmp.to_str().unwrap()),
        None,
        OutputFormat::Text,
    )
    .await
    .expect("run_check with no issues should succeed (exit 0)");

    std::fs::remove_file(&tmp).ok();
}

/// Verify that `run_check` returns a non-zero error when the mock returns an
/// error-severity issue (CI-gate semantics).
#[tokio::test]
async fn run_check_with_errors_returns_nonzero() {
    let server = MockServer::start().await;

    let body = json!({
        "issues": [
            {
                "severity": "SEVERITY_ERROR",
                "message": "Widget 'cpu-chart' references undefined variable 'env'",
                "location": "/sections/0/rows/1/widgets/2"
            }
        ]
    });

    Mock::given(method("POST"))
        .and(path(CHECK_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    let tmp = write_minimal_dashboard();

    let err = run_check(
        &targets,
        Some(tmp.to_str().unwrap()),
        None,
        OutputFormat::Text,
    )
    .await
    .expect_err("run_check should fail (non-zero) when SEVERITY_ERROR issues are present");

    assert!(
        err.to_string().contains("error(s)"),
        "error should mention errors found: {err}"
    );

    std::fs::remove_file(&tmp).ok();
}

/// Verify that `run_check` exits 0 when the mock returns only warning-severity
/// issues (warnings print but do not fail the CI gate).
#[tokio::test]
async fn run_check_with_warnings_exits_zero() {
    let server = MockServer::start().await;

    let body = json!({
        "issues": [
            {
                "severity": "SEVERITY_WARNING",
                "message": "Query uses deprecated function 'timeShift'",
                "location": "/sections/1/rows/0/widgets/0/queries/0"
            }
        ]
    });

    Mock::given(method("POST"))
        .and(path(CHECK_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    let tmp = write_minimal_dashboard();

    run_check(
        &targets,
        Some(tmp.to_str().unwrap()),
        None,
        OutputFormat::Text,
    )
    .await
    .expect("run_check with only warnings should succeed (exit 0)");

    std::fs::remove_file(&tmp).ok();
}

/// Verify that `run_check <dashboard_id>` sends the `dashboardId` oneof field
/// in the request body (not the `dashboard` arm).
#[tokio::test]
async fn run_check_by_id_sends_dashboard_id_in_body() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(CHECK_PATH))
        .and(body_partial_json(json!({ "dashboardId": "dash-abc-123" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "issues": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    run_check(&targets, None, Some("dash-abc-123"), OutputFormat::Text)
        .await
        .expect("run_check by id should succeed");
}

/// Verify that `run_check` renders issues as a JSON array in json output mode.
#[tokio::test]
async fn run_check_json_output_succeeds() {
    let server = MockServer::start().await;

    let body = json!({
        "issues": [
            {
                "severity": "SEVERITY_ERROR",
                "message": "bad widget",
                "location": "/sections/0/rows/0/widgets/0"
            },
            {
                "severity": "SEVERITY_WARNING",
                "message": "deprecated fn",
                "location": "/sections/1/rows/0/widgets/0/queries/0"
            }
        ]
    });

    Mock::given(method("POST"))
        .and(path(CHECK_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    let tmp = write_minimal_dashboard();

    // JSON output path runs even when errors are present; the non-zero exit
    // happens *after* rendering. We assert the render path does not panic and
    // that the call still returns the expected Err (CI gate).
    let err = run_check(
        &targets,
        Some(tmp.to_str().unwrap()),
        None,
        OutputFormat::Json,
    )
    .await
    .expect_err("run_check should fail when SEVERITY_ERROR issues are present");

    assert!(
        err.to_string().contains("error(s)"),
        "error should mention errors found: {err}"
    );

    std::fs::remove_file(&tmp).ok();
}

/// Verify that `run_check` bails with a clear message when neither
/// `--from-file` nor `<dashboard_id>` is supplied (the runtime guard that
/// backs up clap's `conflicts_with`).
#[tokio::test]
async fn run_check_neither_source_fails() {
    let server = MockServer::start().await;
    let target = common::test_target("mock-profile", &server.uri());
    let targets = vec![target];

    let err = run_check(&targets, None, None, OutputFormat::Text)
        .await
        .expect_err("run_check should fail when no source is supplied");

    assert!(
        err.to_string().contains("specify either"),
        "error should direct the user to supply a source: {err}"
    );
}
