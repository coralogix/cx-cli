#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::integrations::run_list;
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_integrations_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "integrations": [
            {
                "integration": {
                    "id": "aws-sns-shipper",
                    "name": "AWS SNS",
                    "tags": ["AWS", "Logs"],
                    "versions": ["0.0.40"],
                    "integrationType": { "cloudformation": {} }
                },
                "errors": []
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/integrations/v1"))
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
        .and(path("/mgmt/openapi/5/integrations/integrations/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "integrations": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list should succeed with empty response");
}
