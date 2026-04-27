mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::slos::{run_get, run_list};
use cx::config::OutputFormat;

#[tokio::test]
async fn list_slos_returns_items_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "slos": [
            {
                "id": "slo-001",
                "name": "API Availability",
                "targetThresholdPercentage": 99.9,
                "sloType": "SLO_TYPE_REQUEST",
                "sloTimeFrame": "SLO_TIME_FRAME_28_DAYS",
                "createTime": "2024-01-01T00:00:00Z"
            },
            {
                "id": "slo-002",
                "name": "Latency SLO",
                "targetThresholdPercentage": 95.0,
                "sloType": "SLO_TYPE_WINDOW",
                "sloTimeFrame": "SLO_TIME_FRAME_7_DAYS"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/slo/slos/v1"))
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
async fn list_slos_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/slo/slos/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"slos": []})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_list(&targets, OutputFormat::Text)
        .await
        .expect("run_list with empty response should succeed");
}

#[tokio::test]
async fn get_slo_by_id() {
    let server = MockServer::start().await;

    let body = json!({
        "slo": {
            "id": "slo-001",
            "name": "API Availability",
            "targetThresholdPercentage": 99.9,
            "sloType": "SLO_TYPE_REQUEST",
            "sloTimeFrame": "SLO_TIME_FRAME_28_DAYS"
        }
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/slo/slos/v1/slo-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_get(&targets, "slo-001", OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}
