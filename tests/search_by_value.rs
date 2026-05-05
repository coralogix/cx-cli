mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::search_by_value;
use coralogix_cli::config::OutputFormat;

fn mock_matches_body() -> serde_json::Value {
    json!({
        "matches": [
            {
                "key_matched": "http.target",
                "value": "/api/v1/payments",
                "similarity_score": 0.94
            },
            {
                "key_matched": "kubernetes.pod.name",
                "value": "payment-api-7d9f",
                "similarity_score": 0.87
            }
        ],
        "total_hits": 2
    })
}

#[tokio::test]
async fn search_by_value_returns_results() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_matches_body()))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_by_value::run(&targets, "payment", "logs", 10, 0, OutputFormat::Json)
        .await
        .expect("search-by-value should succeed");
}

#[tokio::test]
async fn search_by_value_sends_correct_request_body() {
    let server = MockServer::start().await;

    let expected_body = json!({
        "query": "payment",
        "dataset_type": "logs",
        "limit": 10,
        "offset": 0
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_matches_body()))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_by_value::run(&targets, "payment", "logs", 10, 0, OutputFormat::Json)
        .await
        .expect("search-by-value should send correct request body");
}

#[tokio::test]
async fn search_by_value_with_spans_dataset() {
    let server = MockServer::start().await;

    let expected_body = json!({
        "query": "error",
        "dataset_type": "spans",
        "limit": 5,
        "offset": 0
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "matches": [],
            "total_hits": 0
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_by_value::run(&targets, "error", "spans", 5, 0, OutputFormat::Json)
        .await
        .expect("search-by-value with spans dataset should succeed");
}

#[tokio::test]
async fn search_by_value_with_all_dataset() {
    let server = MockServer::start().await;

    let expected_body = json!({
        "query": "kubernetes",
        "dataset_type": "all",
        "limit": 20,
        "offset": 0
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_matches_body()))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_by_value::run(&targets, "kubernetes", "all", 20, 0, OutputFormat::Json)
        .await
        .expect("search-by-value with all dataset should succeed");
}

#[tokio::test]
async fn search_by_value_with_offset_for_pagination() {
    let server = MockServer::start().await;

    let expected_body = json!({
        "query": "payment",
        "dataset_type": "logs",
        "limit": 10,
        "offset": 20
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "matches": [],
            "total_hits": 25
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_by_value::run(&targets, "payment", "logs", 10, 20, OutputFormat::Json)
        .await
        .expect("search-by-value with offset should succeed");
}

#[tokio::test]
async fn search_by_value_clamps_limit_to_100() {
    let server = MockServer::start().await;

    let expected_body = json!({
        "query": "test",
        "dataset_type": "logs",
        "limit": 100,
        "offset": 0
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "matches": [],
            "total_hits": 0
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_by_value::run(&targets, "test", "logs", 500, 0, OutputFormat::Json)
        .await
        .expect("search-by-value should clamp limit to 100");
}

#[tokio::test]
async fn search_by_value_clamps_limit_zero_to_one() {
    let server = MockServer::start().await;

    let expected_body = json!({
        "query": "test",
        "dataset_type": "logs",
        "limit": 1,
        "offset": 0
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "matches": [],
            "total_hits": 0
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_by_value::run(&targets, "test", "logs", 0, 0, OutputFormat::Json)
        .await
        .expect("limit=0 should be clamped to 1");
}

#[tokio::test]
async fn search_by_value_empty_query_returns_error() {
    let target = common::test_target("test-profile", "http://127.0.0.1:1");
    let targets = vec![target];

    let result = search_by_value::run(&targets, "   ", "logs", 10, 0, OutputFormat::Json).await;
    assert!(
        result.is_err(),
        "empty/whitespace query must return Err before any HTTP call"
    );
}

#[tokio::test]
async fn search_by_value_empty_results_text_output() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "matches": [],
            "total_hits": 0
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_by_value::run(&targets, "zzznomatch", "logs", 10, 0, OutputFormat::Text)
        .await
        .expect("search-by-value with empty results should succeed in text mode");
}

#[tokio::test]
async fn search_by_value_agents_output() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_matches_body()))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    search_by_value::run(&targets, "payment", "logs", 10, 0, OutputFormat::Agents)
        .await
        .expect("search-by-value agents output should succeed");
}

#[tokio::test]
async fn search_by_value_multi_profile_merges_results() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    let body_a = json!({
        "matches": [{"key_matched": "http.target", "value": "/api/v1/payments", "similarity_score": 0.94}],
        "total_hits": 1
    });
    let body_b = json!({
        "matches": [{"key_matched": "kubernetes.pod.name", "value": "payment-api-7d9f", "similarity_score": 0.87}],
        "total_hits": 1
    });

    for (server, body) in [(&server_a, &body_a), (&server_b, &body_b)] {
        Mock::given(method("POST"))
            .and(path("/api/v1/search-by-value"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    let target_a = common::test_target("profile-a", &server_a.uri());
    let target_b = common::test_target("profile-b", &server_b.uri());
    let targets = vec![target_a, target_b];

    search_by_value::run(&targets, "payment", "logs", 10, 0, OutputFormat::Json)
        .await
        .expect("search-by-value multi-profile fan-out should succeed");
}

#[tokio::test]
async fn search_by_value_partial_profile_failure_returns_ok() {
    let server_ok = MockServer::start().await;
    let server_fail = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "matches": [{"key_matched": "http.method", "value": "GET", "similarity_score": 0.9}],
            "total_hits": 1
        })))
        .mount(&server_ok)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server_fail)
        .await;

    let targets = vec![
        common::test_target("profile-ok", &server_ok.uri()),
        common::test_target("profile-fail", &server_fail.uri()),
    ];

    search_by_value::run(&targets, "GET", "logs", 10, 0, OutputFormat::Json)
        .await
        .expect("partial failure should return Ok with results from the good profile");
}

#[tokio::test]
async fn search_by_value_all_profiles_fail_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/search-by-value"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let result = search_by_value::run(&targets, "payment", "logs", 10, 0, OutputFormat::Json).await;
    assert!(result.is_err(), "all-profiles-fail must return Err");
}
