mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::dashboards::api::DashboardsApi;
use cx::commands::dashboards::run_catalog;
use cx::config::OutputFormat;

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
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1/catalog"))
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
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1/catalog"))
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
