#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::slos::api::SloStatusQuery;
use coralogix_cli::commands::slos::{run_get, run_list, run_status};
use coralogix_cli::config::OutputFormat;

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

fn status_body() -> serde_json::Value {
    json!({
        "computedAt": "2026-11-17T10:00:00Z",
        "effectiveFrom": "2026-11-01T00:00:00Z",
        "effectiveTo": "2026-11-15T00:00:00Z",
        "lowestCompliance": 0.9995,
        "lowestRemainingErrorBudget": 0.5,
        "effectiveWindowCompleteData": true
    })
}

async fn mount_names(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/slo/slos/v1/all/list"))
        .and(query_param("ids", "slo-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "slos": [{"id": "slo-001", "name": "API Availability"}]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn status_sends_window_slice_and_target() {
    let server = MockServer::start().await;
    mount_names(&server).await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/slo/slos/v1/slo-001/status"))
        .and(query_param("from", "2026-11-01T00:00:00Z"))
        .and(query_param("to", "2026-11-15T00:00:00Z"))
        .and(query_param("grouping_key_value", "service,api"))
        .and(query_param("target_threshold_percentage", "99.5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(status_body()))
        .expect(1)
        .mount(&server)
        .await;

    let query = SloStatusQuery::from_flags(
        None,
        Some("2026-11-01T00:00:42Z"),
        Some("2026-11-15T00:00:00Z"),
        vec!["service,api".to_string()],
        Some(99.5),
    )
    .expect("flags should parse");

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_status(
        &targets,
        &["slo-001".to_string()],
        &query,
        OutputFormat::Json,
    )
    .await
    .expect("run_status should succeed");
}

#[tokio::test]
async fn status_without_flags_leaves_the_window_to_the_server() {
    let server = MockServer::start().await;
    mount_names(&server).await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/slo/slos/v1/slo-001/status"))
        .and(query_param_is_missing("from"))
        .and(query_param_is_missing("to"))
        .and(query_param_is_missing("target_threshold_percentage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(status_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_status(
        &targets,
        &["slo-001".to_string()],
        &SloStatusQuery::default(),
        OutputFormat::Text,
    )
    .await
    .expect("run_status should succeed");
}

#[tokio::test]
async fn status_fails_when_one_slo_fails() {
    let server = MockServer::start().await;
    mount_names(&server).await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/slo/slos/v1/slo-001/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(status_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/slo/slos/v1/slo-missing/status"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "not found"})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let error = run_status(
        &targets,
        &["slo-001".to_string(), "slo-missing".to_string()],
        &SloStatusQuery::default(),
        OutputFormat::Json,
    )
    .await
    .expect_err("one failed SLO fails the command");

    assert!(format!("{error:#}").contains("slo-missing"));
}
