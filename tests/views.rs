mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::views::{run_folders_list, run_list};
use cx::config::OutputFormat;

#[tokio::test]
async fn list_views_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "views": [
            { "id": "v-001", "name": "My View", "folderId": "f-001", "createdAt": "2024-01-01" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/views/views/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}

#[tokio::test]
async fn list_views_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/views/views/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "views": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list should succeed with empty");
}

#[tokio::test]
async fn list_view_folders_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "folders": [
            { "id": "f-001", "name": "Infra" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/views/folders/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_folders_list(&[target], OutputFormat::Json)
        .await
        .expect("run_folders_list should succeed");
}
