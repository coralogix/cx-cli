#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::data_archive::{run_logs_get, run_metrics_get};
use cx::config::OutputFormat;

#[tokio::test]
async fn metrics_get_from_mock() {
    let server = MockServer::start().await;

    let body = json!({ "enabled": false, "bucket": "my-bucket" });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/metrics/data-setup/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_metrics_get(&[target], OutputFormat::Json)
        .await
        .expect("run_metrics_get should succeed");
}

#[tokio::test]
async fn logs_get_from_mock() {
    let server = MockServer::start().await;

    let body = json!({ "target": { "bucket": "logs-archive" } });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/logs/data-setup/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_logs_get(&[target], OutputFormat::Json)
        .await
        .expect("run_logs_get should succeed");
}
