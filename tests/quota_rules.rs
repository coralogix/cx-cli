mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::quota_rules::run_get;
use cx::config::OutputFormat;

#[tokio::test]
async fn quota_rules_get_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "rules": [
            {"id": "rule-001", "name": "Team A Quota", "limit": 100}
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/dataplan/quota-rules/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_get(&targets, OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}
