#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::data_usage::{
    run_logs_count, run_spans_count, run_summary, CountCommandOptions,
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
