#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::dataprime::semantic_search::semantic_field_lookup;
use coralogix_cli::commands::search_fields;
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn semantic_field_search_returns_results() {
    let server = MockServer::start().await;

    let body = json!({
        "results": [
            {
                "path_array": ["$d", "http", "status_code"],
                "description": "HTTP response status code",
                "similarity_score": 0.92,
                "dataset_scope": "USER_DATA",
                "labels": {}
            },
            {
                "path_array": ["$d", "http", "method"],
                "description": "HTTP request method",
                "similarity_score": 0.85,
                "dataset_scope": "USER_DATA",
                "labels": {}
            }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/semantic-search/fields"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_fields::run(&targets, "HTTP status code", "logs", 10, OutputFormat::Json)
        .await
        .expect("search_fields should succeed");
}

#[tokio::test]
async fn semantic_field_search_empty_results() {
    let server = MockServer::start().await;

    let body = json!({ "results": [], "total": 0 });

    Mock::given(method("POST"))
        .and(path("/api/v1/semantic-search/fields"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_fields::run(
        &targets,
        "nonexistent field",
        "logs",
        10,
        OutputFormat::Json,
    )
    .await
    .expect("search_fields with no results should succeed");
}

#[tokio::test]
async fn semantic_field_lookup_serializes_complex_paths() {
    let server = MockServer::start().await;

    let body = json!({
        "results": [
            {
                "path_array": ["$d", "resource.type", "service name"],
                "description": "Resource service name",
                "similarity_score": 0.91,
                "dataset_scope": "USER_DATA",
                "labels": {}
            }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/semantic-search/fields"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let results = semantic_field_lookup(&target.client, "service name", "logs", 10)
        .await
        .expect("semantic_field_lookup should succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].dataprime_path,
        "$d['resource.type']['service name']"
    );
}

#[tokio::test]
async fn semantic_field_search_spans_dataset() {
    let server = MockServer::start().await;

    let body = json!({
        "results": [
            {
                "path_array": ["$d", "traceID"],
                "description": "Distributed trace identifier",
                "similarity_score": 0.88,
                "dataset_scope": "USER_DATA",
                "labels": {}
            }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/semantic-search/fields"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_fields::run(&targets, "trace ID", "spans", 5, OutputFormat::Json)
        .await
        .expect("search_fields on spans dataset should succeed");
}
