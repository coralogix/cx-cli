#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::actions::run_list;
use cx::config::OutputFormat;

#[tokio::test]
async fn list_actions_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "actions": [
            { "id": "act-001", "name": "Slack Alert", "type": "slack", "url": "https://hooks.slack.com/...", "isActive": true }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/actions/actions/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}
