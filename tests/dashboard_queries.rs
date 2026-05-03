mod common;

use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::dashboards;
use cx::config::OutputFormat;

#[tokio::test]
async fn dashboard_search_returns_results() {
    let server = MockServer::start().await;

    let body = json!({
        "results": [
            {
                "query_text": "rate(http_requests_total[5m])",
                "similarity": 0.93,
                "dashboard_name": "API Overview",
                "dashboard_folder": "Engineering",
                "widget_title": "Request Rate",
                "widget_type": "line_chart",
                "query_context": "Monitors request rate",
                "extracted_fields": ["http_requests_total"]
            }
        ],
        "total": 1
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_search(&targets, "http request rate", 10, OutputFormat::Json)
        .await
        .expect("dashboard search should succeed");
}

#[tokio::test]
async fn dashboard_search_empty_results() {
    let server = MockServer::start().await;

    let body = json!({ "results": [], "total": 0 });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_search(&targets, "zzznomatch", 10, OutputFormat::Json)
        .await
        .expect("dashboard search with no results should succeed");
}

#[tokio::test]
async fn dashboard_search_post_body_includes_query_text_and_limit() {
    let server = MockServer::start().await;

    let body = json!({ "results": [], "total": 0 });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .and(query_param("query_text", "error rate by service"))
        .and(query_param("limit", "15"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_search(&targets, "error rate by service", 15, OutputFormat::Json)
        .await
        .expect("dashboard search should send expected JSON body");
}

#[tokio::test]
async fn dashboard_queries_by_field_returns_results() {
    let server = MockServer::start().await;

    let body = json!({
        "queries": [
            {
                "query_text": "filter $d.http.status == 500",
                "dashboard_name": "Error Monitor",
                "widget_title": "5xx Errors",
                "matched_fields": ["$d.http.status"]
            }
        ],
        "total": 1
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/queries/by-field"))
        .and(query_param("field_path", "$d.http.status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_queries_by_field(&targets, "$d.http.status", 10, OutputFormat::Json)
        .await
        .expect("queries_by_field should succeed");
}

#[tokio::test]
async fn dashboard_semantic_search_returns_results() {
    let server = MockServer::start().await;

    let body = json!({
        "results": [
            {
                "dashboard_id": "abc-123",
                "dashboard_name": "Kubernetes Overview",
                "dashboard_folder": "Infrastructure",
                "description": "Pod memory and CPU usage",
                "semantic_description": "Monitors Kubernetes pod resource consumption",
                "widget_count": 8,
                "similarity": 0.91
            }
        ],
        "total": 1
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/semantic-search"))
        .and(query_param("query_text", "kubernetes pod memory usage"))
        .and(query_param("limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_semantic_search(
        &targets,
        "kubernetes pod memory usage",
        10,
        OutputFormat::Json,
    )
    .await
    .expect("dashboard semantic search should succeed");
}

#[tokio::test]
async fn dashboard_queries_by_field_get_includes_limit_query_param() {
    let server = MockServer::start().await;

    let body = json!({ "queries": [], "total": 0 });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/queries/by-field"))
        .and(query_param("field_path", "$d.http.status"))
        .and(query_param("limit", "14"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_queries_by_field(&targets, "$d.http.status", 14, OutputFormat::Json)
        .await
        .expect("queries_by_field should send limit as query parameter");
}

#[tokio::test]
async fn dashboard_semantic_search_encodes_query_text_for_http() {
    let server = MockServer::start().await;

    let body = json!({ "results": [], "total": 0 });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/semantic-search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_semantic_search(&targets, "café & x", 10, OutputFormat::Json)
        .await
        .expect("semantic search with special characters should succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("server should record requests");
    assert_eq!(reqs.len(), 1);
    let query = reqs[0]
        .url
        .query()
        .expect("GET should include query string");
    // "é" must be percent-encoded — the raw byte sequence is %C3%A9
    assert!(
        query.contains("%C3%A9"),
        "unicode in query_text must be percent-encoded in URL: {query:?}"
    );
    assert!(
        query.contains("%26"),
        "ampersand in query_text value must be percent-encoded as %26: {query:?}"
    );
}

#[tokio::test]
async fn dashboard_search_with_agents_runs_toon_encode_path() {
    let server = MockServer::start().await;

    let body = json!({ "results": [], "total": 0 });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_search(&targets, "anything", 10, OutputFormat::Agents)
        .await
        .expect("agents output (TOON) should succeed when HTTP mock returns 200");
}

#[tokio::test]
async fn dashboard_queries_by_field_with_agents_runs_toon_encode_path() {
    let server = MockServer::start().await;

    let body = json!({ "queries": [], "total": 0 });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/queries/by-field"))
        .and(query_param("field_path", "$d.http.status"))
        .and(query_param("limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_queries_by_field(&targets, "$d.http.status", 10, OutputFormat::Agents)
        .await
        .expect("agents output (TOON) should succeed for queries-by-field");
}

#[tokio::test]
async fn dashboard_semantic_search_with_agents_runs_toon_encode_path() {
    let server = MockServer::start().await;

    let body = json!({ "results": [], "total": 0 });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/semantic-search"))
        .and(query_param("query_text", "plain query"))
        .and(query_param("limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    dashboards::run_semantic_search(&targets, "plain query", 10, OutputFormat::Agents)
        .await
        .expect("agents output (TOON) should succeed for semantic search");
}

// ── Multi-target (fan-out) tests ──────────────────────────────────────────────

#[tokio::test]
async fn dashboard_search_multi_profile_merges_results() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    let body_a = json!({
        "results": [{"query_text": "q1", "similarity": 0.9, "dashboard_name": "D1",
                     "dashboard_folder": null, "widget_title": null, "widget_type": null,
                     "query_context": null, "extracted_fields": []}],
        "total": 1
    });
    let body_b = json!({
        "results": [{"query_text": "q2", "similarity": 0.8, "dashboard_name": "D2",
                     "dashboard_folder": null, "widget_title": null, "widget_type": null,
                     "query_context": null, "extracted_fields": []}],
        "total": 1
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body_a))
        .mount(&server_a)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body_b))
        .mount(&server_b)
        .await;

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    dashboards::run_search(&targets, "something", 10, OutputFormat::Json)
        .await
        .expect("multi-profile search should succeed");
}

#[tokio::test]
async fn dashboard_semantic_search_multi_profile_merges_results() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    let body_a = json!({
        "results": [{"dashboard_id": "id-a", "dashboard_name": "DashA",
                     "dashboard_folder": null, "description": null,
                     "semantic_description": null, "widget_count": 3, "similarity": 0.88}],
        "total": 1
    });
    let body_b = json!({ "results": [], "total": 0 });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/semantic-search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body_a))
        .mount(&server_a)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/semantic-search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body_b))
        .mount(&server_b)
        .await;

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    dashboards::run_semantic_search(&targets, "kubernetes", 10, OutputFormat::Json)
        .await
        .expect("multi-profile semantic search should succeed");
}

// ── Error handling tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn dashboard_search_api_500_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result = dashboards::run_search(&targets, "cpu usage", 10, OutputFormat::Json).await;
    assert!(result.is_err(), "500 from all profiles should return Err");
}

#[tokio::test]
async fn dashboard_semantic_search_api_401_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/semantic-search"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"message": "Invalid or expired API key"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result =
        dashboards::run_semantic_search(&targets, "memory usage", 10, OutputFormat::Json).await;
    assert!(result.is_err(), "401 from all profiles should return Err");
}

#[tokio::test]
async fn dashboard_queries_by_field_api_500_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/queries/by-field"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result =
        dashboards::run_queries_by_field(&targets, "$d.status", 10, OutputFormat::Json).await;
    assert!(
        result.is_err(),
        "500 from all profiles should return Err for queries-by-field"
    );
}

#[tokio::test]
async fn dashboard_search_partial_failure_returns_ok() {
    let server_ok = MockServer::start().await;
    let server_fail = MockServer::start().await;

    let body = json!({
        "results": [{"query_text": "q1", "similarity": 0.9, "dashboard_name": "D1",
                     "dashboard_folder": null, "widget_title": null, "widget_type": null,
                     "query_context": null, "extracted_fields": []}],
        "total": 1
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server_ok)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server_fail)
        .await;

    let targets = vec![
        common::test_target("profile-ok", &server_ok.uri()),
        common::test_target("profile-fail", &server_fail.uri()),
    ];

    // One profile fails but one succeeds — should still return Ok (partial results)
    dashboards::run_search(&targets, "cpu", 10, OutputFormat::Json)
        .await
        .expect("partial failure should still return Ok when at least one profile succeeds");
}

// ── Empty query validation tests ─────────────────────────────────────────────

#[tokio::test]
async fn dashboard_search_empty_query_returns_error() {
    let target = common::test_target("test-profile", "http://127.0.0.1:1");
    let targets = vec![target];
    let result = dashboards::run_search(&targets, "", 10, OutputFormat::Json).await;
    assert!(result.is_err(), "empty query_text should return Err");
    assert!(
        result.unwrap_err().to_string().contains("cannot be empty"),
        "error message should mention empty"
    );
}

#[tokio::test]
async fn dashboard_queries_by_field_empty_field_returns_error() {
    let target = common::test_target("test-profile", "http://127.0.0.1:1");
    let targets = vec![target];
    let result = dashboards::run_queries_by_field(&targets, "   ", 10, OutputFormat::Json).await;
    assert!(result.is_err(), "blank field_path should return Err");
}

#[tokio::test]
async fn dashboard_semantic_search_empty_query_returns_error() {
    let target = common::test_target("test-profile", "http://127.0.0.1:1");
    let targets = vec![target];
    let result = dashboards::run_semantic_search(&targets, "", 10, OutputFormat::Json).await;
    assert!(result.is_err(), "empty query_text should return Err");
}

#[tokio::test]
async fn dashboard_queries_by_field_multi_profile_merges_results() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    let body_a = json!({
        "queries": [{"query_text": "q1", "dashboard_name": "D1", "widget_title": "W1", "matched_fields": ["$d.a"]}],
        "total": 1
    });
    let body_b = json!({ "queries": [], "total": 0 });

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/queries/by-field"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body_a))
        .mount(&server_a)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/queries/by-field"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body_b))
        .mount(&server_b)
        .await;

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    dashboards::run_queries_by_field(&targets, "$d.field", 10, OutputFormat::Json)
        .await
        .expect("multi-profile queries-by-field should succeed");
}

#[tokio::test]
async fn dashboard_search_all_profiles_fail_returns_error() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server_a)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/olly-kb/dashboards/queries/search"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server_b)
        .await;

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    let result = dashboards::run_search(&targets, "cpu", 10, OutputFormat::Json).await;
    assert!(
        result.is_err(),
        "when every profile fails, run_search should return Err for CI/scripts"
    );
}
