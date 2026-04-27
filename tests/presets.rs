mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::presets::run_list;
use cx::config::OutputFormat;

#[tokio::test]
async fn list_presets_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "presets": [
            { "id": "preset-001", "name": "Default Slack", "connectorType": "CONNECTOR_TYPE_SLACK", "isDefault": true, "isCustom": false }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/notifications/notification-center/v1/presets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}
