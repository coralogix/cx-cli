#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::webhooks::{run_list, run_types};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_webhooks_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "webhooks": [
            { "id": "wh-001", "name": "Slack Notify", "type": "slack", "url": "https://hooks.slack.com/abc" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/webhooks/v1"))
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
async fn list_webhooks_with_trailing_slash_endpoint() {
    // Regression: an endpoint with a trailing slash must not produce a `//` in
    // the request path. The mgmt OpenAPI gateway rejects `//mgmt/openapi/...`
    // with 400 Bad Request ("OpenAPI schema"), which is how this surfaced.
    let server = MockServer::start().await;

    let body = json!({
        "webhooks": [
            { "id": "wh-001", "name": "Slack Notify", "type": "slack", "url": "https://hooks.slack.com/abc" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/webhooks/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let base_with_slash = format!("{}/", server.uri());
    let target = common::test_target("test-profile", &base_with_slash);
    run_list(&[target], OutputFormat::Json)
        .await
        .expect("run_list should succeed even with a trailing-slash endpoint");
}

#[tokio::test]
async fn list_webhook_types_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/webhook-types/v1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"types": ["slack", "pagerduty"]})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_types(&[target], OutputFormat::Json)
        .await
        .expect("run_types should succeed");
}
