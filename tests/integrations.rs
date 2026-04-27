mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::integrations::run_list;
use cx::config::OutputFormat;

#[tokio::test]
async fn list_integrations_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "deployments": [
            { "id": "int-001", "name": "AWS Integration", "type": "aws", "status": "active", "version": 1 }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/integrations/integrations/v1"))
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
async fn list_integrations_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/integrations/integrations/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "deployments": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list should succeed with empty response");
}
