mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::e2m::{run_get, run_list};
use cx::config::OutputFormat;

#[tokio::test]
async fn list_e2m_returns_items_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "e2m": [
            {
                "id": "e2m-001",
                "name": "Error Count Metric",
                "type": "E2M_TYPE_LOGS2METRICS",
                "metricName": "error_count_total",
                "createTime": "2024-01-01T00:00:00Z",
                "isActive": true
            },
            {
                "id": "e2m-002",
                "name": "Span Duration",
                "type": "E2M_TYPE_SPANS2METRICS",
                "metricName": "span_duration_seconds"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/e2m/e2m/v2"))
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
async fn get_e2m_by_id() {
    let server = MockServer::start().await;

    let body = json!({
        "e2m": {
            "id": "e2m-001",
            "name": "Error Count Metric",
            "metricName": "error_count_total"
        }
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/e2m/e2m/v2/e2m-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_get(&targets, "e2m-001", OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}
