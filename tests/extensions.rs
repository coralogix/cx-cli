mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::extensions::{run_deployed, run_list};
use cx::config::OutputFormat;

#[tokio::test]
async fn list_extensions_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "extensions": [
            { "id": "ext-001", "name": "AWS CloudWatch", "version": "1.0.0", "deployed": false }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/extensions/extensions/v1"))
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
async fn list_deployed_extensions_from_mock() {
    let server = MockServer::start().await;

    let body = json!({ "deployedExtensions": [] });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/extensions/extensions/v1/deployed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_deployed(&[target], OutputFormat::Text)
        .await
        .expect("run_deployed should succeed");
}
