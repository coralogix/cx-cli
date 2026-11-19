#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::webhooks::{run_create, run_list, run_test, run_types, run_update};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_webhooks_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "deployed": [
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
        "deployed": [
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
async fn create_webhook_accepts_bare_response() {
    // FORGE-696: the live API returns the created webhook bare (`{"id": ...}`),
    // with no `{"webhook": {...}}` envelope. `create` must not deserialize into
    // a shape that requires one. The assertion that the row actually reaches
    // stdout (rather than being silently dropped) needs captured output, so it
    // lives in tests/console_urls/main.rs; what this pins is the request
    // contract - POST to the webhooks endpoint, payload forwarded verbatim.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/integrations/webhooks/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "wh-002" })))
        .expect(1)
        .mount(&server)
        .await;

    // The real outgoing-webhook payload nests everything under `data`.
    let file_path = std::env::temp_dir().join("cx_test_webhooks_create_bare.json");
    std::fs::write(
        &file_path,
        r#"{"data": {"name": "Slack Notify", "type": "slack"}}"#,
    )
    .unwrap();

    let target = common::test_target("test-profile", &server.uri());
    let result = run_create(&[target], file_path.to_str().unwrap(), OutputFormat::Json).await;

    let _ = std::fs::remove_file(&file_path);
    result.expect("run_create should succeed against a bare create response");
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

#[tokio::test]
async fn update_webhook_puts_collection_with_id_in_body() {
    // The live API answers `PUT /webhooks/v1/{id}` with 405; the replace is
    // `PUT /webhooks/v1` with `{"id": ..., "data": {...}}`. The id on the
    // command line must win over whatever the file says.
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/integrations/webhooks/v1"))
        .and(body_json(json!({
            "id": "wh-007",
            "data": { "name": "Renamed", "type": "GENERIC", "url": "https://example.com/x" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let file_path = std::env::temp_dir().join("cx_test_webhooks_update.json");
    std::fs::write(
        &file_path,
        r#"{"id": "stale", "data": {"name": "Renamed", "type": "GENERIC", "url": "https://example.com/x"}}"#,
    )
    .unwrap();

    let target = common::test_target("test-profile", &server.uri());
    let result = run_update(
        &[target],
        "wh-007",
        file_path.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await;

    let _ = std::fs::remove_file(&file_path);
    result.expect("run_update should PUT the collection with the id in the body");
}

#[tokio::test]
async fn test_webhook_is_a_get() {
    // `POST /webhooks/v1/{id}/test` is a 405 on the live API; the test
    // trigger is a GET whose 200 body carries success or failure.
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/webhooks/v1/wh-007/test"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "failure": { "errorMessage": "Testing webhook failed" } })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_test(&[target], "wh-007", OutputFormat::Json)
        .await
        .expect("run_test should succeed on a 200 with a failure payload");
}
