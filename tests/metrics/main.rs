#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::metrics::{run_get_labels, run_query, run_query_range, run_search};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn instant_query_returns_samples() {
    let server = MockServer::start().await;

    let body = json!({
        "status": "success",
        "data": {
            "resultType": "vector",
            "result": [
                {
                    "metric": { "__name__": "up", "instance": "localhost:9090" },
                    "value": [1719000000, "1"]
                },
                {
                    "metric": { "__name__": "up", "instance": "localhost:9091" },
                    "value": [1719000000, "0"]
                }
            ]
        }
    });

    Mock::given(method("GET"))
        .and(path("/metrics/api/v1/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_query(&targets, "up", None, OutputFormat::Json)
        .await
        .expect("run_query (instant) should succeed");
}

#[tokio::test]
async fn range_query_returns_series() {
    let server = MockServer::start().await;

    let body = json!({
        "status": "success",
        "data": {
            "resultType": "matrix",
            "result": [
                {
                    "metric": { "__name__": "http_requests_total", "method": "GET" },
                    "values": [
                        [1719000000, "100"],
                        [1719000060, "105"],
                        [1719000120, "112"]
                    ]
                }
            ]
        }
    });

    Mock::given(method("GET"))
        .and(path("/metrics/api/v1/query_range"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_query_range(
        &targets,
        "http_requests_total",
        "2024-06-22T00:00:00Z",
        "2024-06-22T01:00:00Z",
        "60s",
        OutputFormat::Json,
    )
    .await
    .expect("run_query_range should succeed");
}

#[tokio::test]
async fn search_by_name_pattern() {
    let server = MockServer::start().await;

    let body = json!({
        "status": "success",
        "data": [
            "http_requests_total",
            "http_request_duration_seconds",
            "http_response_size_bytes"
        ]
    });

    Mock::given(method("GET"))
        .and(path("/metrics/api/v1/label/__name__/values"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_search(&targets, Some("http_*"), None, OutputFormat::Json)
        .await
        .expect("run_search by name should succeed");
}

#[tokio::test]
async fn search_by_description() {
    let server = MockServer::start().await;

    let body = json!({
        "results": [
            {
                "metric_name": "http_requests_total",
                "description": "Total number of HTTP requests",
                "metric_type": "counter",
                "metric_suffixes": ["_total"],
                "similarity_score": 0.95
            }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/semantic-search/metrics"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_search(
        &targets,
        None,
        Some("total HTTP requests"),
        OutputFormat::Json,
    )
    .await
    .expect("run_search by description should succeed");
}

#[tokio::test]
async fn get_labels_for_metric() {
    let server = MockServer::start().await;

    let body = json!({
        "status": "success",
        "data": ["__name__", "instance", "job", "method", "status"]
    });

    Mock::given(method("GET"))
        .and(path("/metrics/api/v1/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_get_labels(&targets, "http_requests_total", OutputFormat::Json)
        .await
        .expect("run_get_labels should succeed");
}
