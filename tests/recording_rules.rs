mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::recording_rules::{run_get, run_list};
use cx::config::OutputFormat;

#[tokio::test]
async fn list_recording_rules_returns_items_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "groups": [
            {
                "id": "rr-001",
                "name": "Error Rate Rules",
                "interval": "60s",
                "rules": [
                    {"record": "job:http_errors:rate5m", "expr": "rate(http_errors_total[5m])"}
                ],
                "createdAt": "2024-01-01T00:00:00Z"
            },
            {
                "id": "rr-002",
                "name": "Latency Rules",
                "interval": "30s",
                "rules": []
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/recording-rules/recording-rules/v1"))
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
async fn get_recording_rule_by_id() {
    let server = MockServer::start().await;

    let body = json!({
        "group": {
            "id": "rr-001",
            "name": "Error Rate Rules",
            "interval": "60s",
            "rules": [
                {"record": "job:http_errors:rate5m", "expr": "rate(http_errors_total[5m])"}
            ]
        }
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/recording-rules/recording-rules/v1/rr-001",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_get(&targets, "rr-001", OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}
