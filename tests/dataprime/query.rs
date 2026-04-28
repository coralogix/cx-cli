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
        Tier::FrequentSearch,
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
        Tier::FrequentSearch,
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
        Tier::FrequentSearch,
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
        Tier::FrequentSearch,
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
        Tier::FrequentSearch,
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
        Tier::FrequentSearch,
        OutputFormat::Json,
        None,
        "/tmp",
        None,
    )
    .await
    .expect("multi-profile fan-out should succeed");
}
