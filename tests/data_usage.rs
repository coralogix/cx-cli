mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::data_usage::run_summary;
use cx::config::OutputFormat;

#[tokio::test]
async fn data_usage_summary_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "totalGb": 42.5,
        "logsGb": 30.0,
        "spansGb": 12.5
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataplans/data-usage/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_summary(&targets, None, None, OutputFormat::Json)
        .await
        .expect("run_summary should succeed");
}
