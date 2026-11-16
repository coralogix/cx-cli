#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::datasets;
use coralogix_cli::config::OutputFormat;

fn sample_response() -> serde_json::Value {
    json!({
        "datasets": [
            {
                "teamId": 17266,
                "dataspace": "default",
                "dataset": "logs",
                "version": 1,
                "pillar": "logs",
                "entityType": "logs"
            },
            {
                "teamId": 17266,
                "dataspace": "default",
                "dataset": "rum.events",
                "version": 1,
                "pillar": "logs",
                "entityType": "browserLogs/v2"
            },
            {
                "teamId": 17266,
                "dataspace": "system",
                "dataset": "labs.limitViolations",
                "version": 1,
                "pillar": "logs",
                "entityType": "labsLimitViolations"
            }
        ]
    })
}

#[tokio::test]
async fn list_returns_datasets_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dataengine/datasets/v1"))
        .and(body_partial_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_response()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    datasets::run_list(&targets, None, OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}

#[tokio::test]
async fn list_with_filter_sends_dpxl_filter_body() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dataengine/datasets/v1"))
        .and(body_partial_json(
            json!({ "dpxlFilter": { "value": "<v1> $d.pillar == 'logs'" } }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_response()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    datasets::run_list(
        &targets,
        Some("<v1> $d.pillar == 'logs'"),
        OutputFormat::Json,
    )
    .await
    .expect("run_list with filter should succeed");
}

#[tokio::test]
async fn list_empty_response() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dataengine/datasets/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    datasets::run_list(&targets, None, OutputFormat::Json)
        .await
        .expect("run_list with empty response should succeed");
}

#[tokio::test]
async fn list_surfaces_api_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dataengine/datasets/v1"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "code": 400,
            "message": "Bad Request: Invalid DPXL filter: expected version tag"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let result = datasets::run_list(&targets, Some("not-a-dpxl-filter"), OutputFormat::Json).await;
    assert!(
        result.is_err(),
        "a 400 from the API must surface as an error"
    );
}
