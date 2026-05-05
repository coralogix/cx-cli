#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::connectors::{run_list, run_types};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_connectors_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "connectors": [
            { "id": "conn-001", "name": "Slack", "type": "CONNECTOR_TYPE_SLACK", "enabled": true }
        ]
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/connectors",
        ))
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
async fn get_connector_types_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/connectors/types/summaries",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"types": []})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_types(&[target], OutputFormat::Json)
        .await
        .expect("run_types should succeed");
}
