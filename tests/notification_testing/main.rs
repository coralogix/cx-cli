#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::notification_testing::run_test_connector;
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn test_connector_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/connectors:testConfig",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true})))
        .expect(1)
        .mount(&server)
        .await;

    let tmpfile = std::env::temp_dir().join("cx_test_connector.json");
    std::fs::write(
        &tmpfile,
        r#"{"type":"slack","url":"https://hooks.slack.com/test"}"#,
    )
    .unwrap();

    let target = common::test_target("test-profile", &server.uri());
    run_test_connector(&[target], tmpfile.to_str().unwrap(), OutputFormat::Json)
        .await
        .expect("run_test_connector should succeed");

    let _ = std::fs::remove_file(tmpfile);
}
