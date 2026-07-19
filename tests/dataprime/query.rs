#[path = "../common/mod.rs"]
mod common;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::dataprime::run_query;
use coralogix_cli::commands::logs;
use coralogix_cli::commands::spans;
use coralogix_cli::config::OutputFormat;
use coralogix_cli::Tier;

/// Build a realistic NDJSON response string for the dataprime query endpoint.
fn make_ndjson_response(rows: &[&str]) -> String {
    let mut lines = vec![r#"{"queryId":{"queryId":"test-query-id"}}"#.to_string()];
    if !rows.is_empty() {
        let results: Vec<String> = rows.iter().map(|r| r.to_string()).collect();
        lines.push(format!(
            r#"{{"result":{{"results":[{}]}}}}"#,
            results.join(",")
        ));
    }
    lines.join("\n")
}

#[tokio::test]
async fn dataprime_query_with_log_results() {
    let server = MockServer::start().await;

    let ndjson = make_ndjson_response(&[
        r#"{"metadata":[{"key":"severity","value":"5"},{"key":"timestamp","value":"2024-06-22T10:00:00Z"}],"labels":[{"key":"applicationname","value":"api"}],"userData":"{\"message\":\"connection timeout\"}"}"#,
        r#"{"metadata":[{"key":"severity","value":"3"},{"key":"timestamp","value":"2024-06-22T10:00:01Z"}],"labels":[{"key":"applicationname","value":"api"}],"userData":"{\"message\":\"request completed\"}"}"#,
    ]);

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_query(
        &targets,
        "source logs",
        "logs",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Json,
        None,
        "/tmp",
        None,
    )
    .await
    .expect("dataprime query should succeed");
}

#[tokio::test]
async fn dataprime_aggregate_query() {
    let server = MockServer::start().await;

    let ndjson = make_ndjson_response(&[
        r#"{"metadata":[],"labels":[],"userData":"{\"region\":\"us1\",\"total_logs\":16}"}"#,
        r#"{"metadata":[],"labels":[],"userData":"{\"region\":\"us2\",\"total_logs\":20}"}"#,
    ]);

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_query(
        &targets,
        "source logs | groupby $l.region aggregate count() as total_logs",
        "logs",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Json,
        None,
        "/tmp",
        None,
    )
    .await
    .expect("aggregate query should succeed");
}

#[tokio::test]
async fn dataprime_query_with_warning() {
    let server = MockServer::start().await;

    let ndjson = [
        r#"{"queryId":{"queryId":"test-query-id"}}"#,
        r#"{"result":{"results":[{"metadata":[],"labels":[],"userData":"{}"}]}}"#,
        r#"{"warning":{"compileWarning":{"warningMessage":"query scanned too many rows"}}}"#,
    ]
    .join("\n");

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_query(
        &targets,
        "source logs",
        "logs",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Json,
        None,
        "/tmp",
        None,
    )
    .await
    .expect("query with warning should succeed");
}

#[tokio::test]
async fn logs_command_delegates_to_dataprime() {
    let server = MockServer::start().await;

    let ndjson = make_ndjson_response(&[
        r#"{"metadata":[{"key":"severity","value":"3"},{"key":"timestamp","value":"2024-06-22T10:00:00Z"}],"labels":[],"userData":"{\"message\":\"hello world\"}"}"#,
    ]);

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    logs::run(
        &targets,
        "source logs",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Json,
        None,
        "/tmp",
    )
    .await
    .expect("logs::run should succeed");
}

#[tokio::test]
async fn spans_command_delegates_to_dataprime() {
    let server = MockServer::start().await;

    let ndjson = make_ndjson_response(&[
        r#"{"metadata":[{"key":"duration","value":"5000"}],"labels":[{"key":"operationName","value":"GET /api/health"},{"key":"serviceName","value":"gateway"}],"userData":"{\"traceID\":\"abc123\",\"spanID\":\"span-1\"}"}"#,
    ]);

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    spans::run(
        &targets,
        "source spans",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Json,
        None,
        "/tmp",
    )
    .await
    .expect("spans::run should succeed");
}

#[tokio::test]
async fn multi_profile_fan_out_tags_rows() {
    let server = MockServer::start().await;

    let ndjson = make_ndjson_response(&[
        r#"{"metadata":[{"key":"severity","value":"3"}],"labels":[],"userData":"{\"message\":\"from profile\"}"}"#,
    ]);

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .mount(&server)
        .await;

    let target1 = common::test_target("prod", &server.uri());
    let target2 = common::test_target("staging", &server.uri());
    let targets = vec![target1, target2];

    run_query(
        &targets,
        "source logs",
        "logs",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Json,
        None,
        "/tmp",
        None,
    )
    .await
    .expect("multi-profile fan-out should succeed");
}

#[tokio::test]
async fn dataprime_query_all_profiles_failing_returns_err() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(500).set_body_string(
            r#"{"message":"Query execution failed: java.lang.OutOfMemoryError: Java heap space"}"#,
        ))
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result = run_query(
        &targets,
        "source logs",
        "logs",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Text,
        None,
        "/tmp",
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "a total query failure must surface as an error, not a silent empty result"
    );
}

#[tokio::test]
async fn dataprime_query_ndjson_unrecognized_envelope_returns_err() {
    let server = MockServer::start().await;

    // Simulates a backend that dies mid-stream (e.g. a real OOM) and emits an
    // envelope this client doesn't recognize, after already sending a 200.
    let ndjson = [
        r#"{"queryId":{"queryId":"test-query-id"}}"#,
        r#"{"error":{"message":"engine out of memory"}}"#,
    ]
    .join("\n");

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result = run_query(
        &targets,
        "source logs",
        "logs",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Text,
        None,
        "/tmp",
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "a mid-stream NDJSON error envelope must surface as an error, not a silent empty result"
    );
}

#[tokio::test]
async fn dataprime_query_non_completed_statistics_status_returns_err() {
    let server = MockServer::start().await;

    // Real Coralogix behavior (discovered via live reproduction): every query
    // ends with a `statistics` envelope; a non-"COMPLETED" status is how the
    // engine reports a failure that happens after the 200 has already been
    // sent (e.g. a real OOM mid-execution).
    let ndjson = [
        r#"{"queryId":{"queryId":"test-query-id"}}"#,
        r#"{"statistics":{"status":"FAILED","outputRowCount":"0"}}"#,
    ]
    .join("\n");

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result = run_query(
        &targets,
        "source logs",
        "logs",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Text,
        None,
        "/tmp",
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "a non-COMPLETED statistics status must surface as an error, not a silent empty result"
    );
}

#[tokio::test]
async fn dataprime_query_completed_statistics_with_zero_rows_succeeds() {
    let server = MockServer::start().await;

    // A query that legitimately matches nothing still ends with a COMPLETED
    // statistics line and no `result` line - must NOT be treated as an error.
    let ndjson = [
        r#"{"queryId":{"queryId":"test-query-id"}}"#,
        r#"{"statistics":{"status":"COMPLETED","outputRowCount":"0"}}"#,
    ]
    .join("\n");

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_query(
        &targets,
        "source logs",
        "logs",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Text,
        None,
        "/tmp",
        None,
    )
    .await
    .expect("a legitimately empty result with COMPLETED status must still succeed");
}
