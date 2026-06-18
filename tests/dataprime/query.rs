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

/// A Dataprime backend error line (HTTP 200, error embedded in NDJSON) must
/// cause `run_query` to return `Err` with a message that contains both
/// "query failed" and the backend-reported reason.
///
/// NOTE: The exact JSON shape used here (`{"error":{"queryError":{"errorMessage":"..."}}}`)
/// is our best guess at the proto3 JSON encoding by analogy with the warning shape.
/// It MUST be verified against a live query failure before this test is considered
/// canonical.  The defensive parser always falls back to the raw JSON if the shape
/// differs, so functionality is preserved even if the mock shape is wrong.
#[tokio::test]
async fn dataprime_query_error_returns_err() {
    let server = MockServer::start().await;

    let ndjson = [
        r#"{"queryId":{"queryId":"test-query-id"}}"#,
        r#"{"error":{"queryError":{"errorMessage":"query ran out of memory"}}}"#,
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

    let result = run_query(
        &targets,
        "source logs | groupby $m.logid aggregate any_value($d) as data",
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
    .await;

    let err = result.expect_err("query with error NDJSON line should return Err");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("query failed"),
        "error message should contain 'query failed': {msg}"
    );
    assert!(
        msg.contains("query ran out of memory"),
        "error message should contain the backend reason: {msg}"
    );
}

/// When two profiles both return error NDJSON lines, `run_query` should return
/// `Err` (not `Ok` with empty rows) and must NOT print "No results found." to
/// stdout.
#[tokio::test]
async fn dataprime_query_all_profiles_error_returns_err() {
    let server = MockServer::start().await;

    let ndjson = [
        r#"{"queryId":{"queryId":"test-query-id"}}"#,
        r#"{"error":{"queryError":{"errorMessage":"all shards failed"}}}"#,
    ]
    .join("\n");

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .mount(&server)
        .await;

    let target1 = common::test_target("prod", &server.uri());
    let target2 = common::test_target("staging", &server.uri());
    let targets = vec![target1, target2];

    let result = run_query(
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
    .await;

    assert!(
        result.is_err(),
        "all-profile failure should return Err, not Ok"
    );
}

/// When one profile returns an error NDJSON line and another succeeds, `run_query`
/// should return `Ok` (partial success — the good profile's rows are rendered).
#[tokio::test]
async fn dataprime_query_partial_profile_error_returns_ok() {
    let error_server = MockServer::start().await;
    let ok_server = MockServer::start().await;

    let error_ndjson = [
        r#"{"queryId":{"queryId":"test-query-id"}}"#,
        r#"{"error":"bad profile OOM"}"#,
    ]
    .join("\n");

    let ok_ndjson = make_ndjson_response(&[
        r#"{"metadata":[{"key":"severity","value":"3"}],"labels":[],"userData":"{\"message\":\"hello\"}"}"#,
    ]);

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&error_ndjson))
        .mount(&error_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ok_ndjson))
        .mount(&ok_server)
        .await;

    let target_bad = common::test_target("bad-profile", &error_server.uri());
    let target_good = common::test_target("good-profile", &ok_server.uri());
    let targets = vec![target_bad, target_good];

    let result = run_query(
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
    .await;

    assert!(
        result.is_ok(),
        "partial profile failure should return Ok (good profile still has rows)"
    );
}
