#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::e2m::{run_get, run_list};
use coralogix_cli::config::OutputFormat;

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
        .and(path("/mgmt/openapi/5/events2metrics/events2metrics/v2"))
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
        .and(path(
            "/mgmt/openapi/5/events2metrics/events2metrics/v2/e2m-001",
        ))
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

// ── limits / labels-cardinality against the real v2 wire shapes ─────────────

/// `GET /limits/v2` nests the definition cap under `metricsLimit`.
#[tokio::test]
async fn limits_reads_nested_metrics_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/events2metrics/limits/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "companyId": "4019663",
            "labelsLimit": 6,
            "permutationsLimit": { "limit": 10000000, "used": 0 },
            "metricsLimit": { "limit": 300, "used": 2 }
        })))
        .expect(2)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    coralogix_cli::commands::e2m::run_limits(std::slice::from_ref(&target), OutputFormat::Json)
        .await
        .expect("run_limits json should succeed");
    coralogix_cli::commands::e2m::run_limits(&[target], OutputFormat::Text)
        .await
        .expect("run_limits text should succeed");
}

/// The cardinality endpoint requires a query; the CLI sends a match-all
/// logs query and reads the `permutations` list.
#[tokio::test]
async fn labels_cardinality_sends_a_query() {
    use wiremock::matchers::query_param;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/events2metrics/labels/v2/cardinalities",
        ))
        .and(query_param("logs_query.lucene", "*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "permutations": [{ "day": "2026-11-10", "permutations": 3 }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    coralogix_cli::commands::e2m::run_labels_cardinality(&[target], OutputFormat::Json)
        .await
        .expect("run_labels_cardinality should succeed");
}
