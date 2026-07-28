#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::data_usage::{
    run_capabilities, run_logs_count, run_query, run_spans_count, run_summary, CountCommandOptions,
};
use coralogix_cli::config::OutputFormat;

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
        .and(header("Accept", "text/event-stream"))
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

#[tokio::test]
async fn data_usage_capabilities_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataplan/data-usage/v1/capabilities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "supportedLabels": [{"key": "applicationName"}],
            "supportedMeasurements": [{"kind": "LOGS", "unit": "BYTES"}],
            "maxGroupByLabels": 3,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_capabilities(&targets, OutputFormat::Json)
        .await
        .expect("run_capabilities should succeed");
}

#[tokio::test]
async fn data_usage_query_forwards_json_body_from_file() {
    let server = MockServer::start().await;
    let query = json!({
        "daily": {"relativeRange": "DAILY_RELATIVE_RANGE_LAST_7_DAYS"},
        "groupBy": {"keys": ["applicationName"]},
        "limit": {"perBucket": 10},
    });

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dataplan/data-usage/v1/query"))
        .and(body_json(&query))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "queryRange": {
                "start": "2026-07-01T00:00:00Z",
                "end": "2026-07-08T00:00:00Z",
            },
            "buckets": [],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file_path = std::env::temp_dir().join(format!(
        "cx-data-usage-query-test-{}.json",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after Unix epoch")
            .as_nanos()
    ));
    std::fs::write(&file_path, serde_json::to_vec(&query).unwrap()).unwrap();

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let result = run_query(
        &targets,
        file_path.to_str().expect("temporary path is valid UTF-8"),
        OutputFormat::Json,
    )
    .await;
    std::fs::remove_file(file_path).unwrap();

    result.expect("run_query should succeed");
}

#[tokio::test]
async fn run_logs_count_forwards_time_and_query_params() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataplans/data-usage/v2/logs/count"))
        .and(header("Accept", "text/event-stream"))
        .and(query_param(
            "date_range.fromDate",
            "2024-01-01T00:00:00.000Z",
        ))
        .and(query_param("date_range.toDate", "2024-01-02T00:00:00.000Z"))
        .and(query_param("resolution", "1h"))
        .and(query_param("application_aggregation", "true"))
        .and(query_param("filters.applicationName", "api"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{\"logsCount\":[]}\n"))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];
    let params = vec!["filters.applicationName=api".to_string()];

    run_logs_count(
        &targets,
        CountCommandOptions {
            start: Some("2024-01-01T00:00:00Z"),
            end: Some("2024-01-02T00:00:00Z"),
            resolution: None,
            subsystem_aggregation: false,
            application_aggregation: true,
            extra_params: &params,
            output: OutputFormat::Json,
        },
    )
    .await
    .expect("run_logs_count should forward query params");
}

#[tokio::test]
async fn run_spans_count_forwards_time_and_query_params() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataplans/data-usage/v2/spans/count"))
        .and(header("Accept", "text/event-stream"))
        .and(query_param(
            "date_range.fromDate",
            "2024-01-01T00:00:00.000Z",
        ))
        .and(query_param("date_range.toDate", "2024-01-02T00:00:00.000Z"))
        .and(query_param("resolution", "6h"))
        .and(query_param("subsystem_aggregation", "true"))
        .and(query_param("filters.subsystemName", "worker"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("{\"result\":{\"spansCount\":[]}}\n"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];
    let params = vec!["filters.subsystemName=worker".to_string()];

    run_spans_count(
        &targets,
        CountCommandOptions {
            start: Some("2024-01-01T00:00:00Z"),
            end: Some("2024-01-02T00:00:00Z"),
            resolution: Some("6h"),
            subsystem_aggregation: true,
            application_aggregation: false,
            extra_params: &params,
            output: OutputFormat::Json,
        },
    )
    .await
    .expect("run_spans_count should forward query params");
}
