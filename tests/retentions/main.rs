#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::retentions::{run_list, run_status};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn retentions_list_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "retentions": [
            {"id": "ret-001", "name": "Hot Storage", "retentionDays": 30, "enabled": true}
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/dataengine/retention-tags/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_list(&targets, OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}

#[tokio::test]
async fn retentions_status_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/latest/dataengine/retention-tags/v1/enabled",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"enabled": true})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_status(&targets, OutputFormat::Json)
        .await
        .expect("run_status should succeed");
}
