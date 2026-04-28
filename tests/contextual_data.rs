mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::contextual_data::run_list;
use cx::config::OutputFormat;

#[tokio::test]
async fn list_contextual_data_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "integrations": [
            { "id": "cd-001", "name": "GitHub Commits", "type": "github", "status": "active" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/contextual-data/v1"))
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
async fn list_contextual_data_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/contextual-data/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "integrations": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list should succeed with empty");
}
