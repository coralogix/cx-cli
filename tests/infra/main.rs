#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::infra::{run_health_history, run_list, run_raw_data, run_types};
use coralogix_cli::config::OutputFormat;

const BASE: &str = "/mgmt/api/infrastructure/resources/v1";

fn types_body() -> serde_json::Value {
    json!({
        "resourceTypes": [
            {
                "categoryType": { "category": "Hosts", "type": "EC2_Instances" },
                "resourceType": "aws_ec2_instance",
                "label": "EC2 Instances"
            }
        ]
    })
}

#[tokio::test]
async fn types_returns_mappings_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/types")))
        .respond_with(ResponseTemplate::new(200).set_body_json(types_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_types(&targets, OutputFormat::Json)
        .await
        .expect("run_types should succeed");
}

#[tokio::test]
async fn types_merges_multiple_profiles() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    for server in [&server_a, &server_b] {
        Mock::given(method("GET"))
            .and(path(format!("{BASE}/types")))
            .respond_with(ResponseTemplate::new(200).set_body_json(types_body()))
            .expect(1)
            .mount(server)
            .await;
    }

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    run_types(&targets, OutputFormat::Json)
        .await
        .expect("run_types should merge both profiles");
}

#[tokio::test]
async fn types_tolerates_one_failing_profile() {
    let server_ok = MockServer::start().await;
    let server_err = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/types")))
        .respond_with(ResponseTemplate::new(200).set_body_json(types_body()))
        .expect(1)
        .mount(&server_ok)
        .await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/types")))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(json!({ "error": "backend exploded" })),
        )
        .expect(1)
        .mount(&server_err)
        .await;

    let targets = vec![
        common::test_target("profile-ok", &server_ok.uri()),
        common::test_target("profile-err", &server_err.uri()),
    ];

    run_types(&targets, OutputFormat::Json)
        .await
        .expect("one failing profile should be non-fatal");
}

#[tokio::test]
async fn types_errors_when_all_profiles_fail() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/types")))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({ "error": "boom" })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let result = run_types(&targets, OutputFormat::Json).await;
    assert!(result.is_err(), "all profiles failing should be an error");
}

#[tokio::test]
async fn list_sends_all_query_params() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(BASE))
        .and(query_param("category", "Hosts"))
        .and(query_param("type", "EC2_Instances"))
        .and(query_param("nameFilter", "web"))
        .and(query_param("scopeFilter.service", "checkout"))
        .and(query_param("scopeFilter.environment", "prod"))
        .and(query_param("startRow", "100"))
        .and(query_param("endRow", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resources": [
                {
                    "resourceId": "1001234:host_id=i-abc123",
                    "name": "web-server-1",
                    "columns": { "region": "us-east-1" }
                }
            ],
            "totalCount": 1
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let scope = vec![
        "service=checkout".to_string(),
        "environment=prod".to_string(),
    ];

    run_list(
        &targets,
        "Hosts",
        "EC2_Instances",
        Some("web"),
        &scope,
        Some(100),
        Some(200),
        OutputFormat::Json,
    )
    .await
    .expect("run_list should send all query params");
}

#[tokio::test]
async fn list_omits_optional_query_params() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(BASE))
        .and(query_param("category", "Hosts"))
        .and(query_param("type", "EC2_Instances"))
        .and(query_param_is_missing("nameFilter"))
        .and(query_param_is_missing("startRow"))
        .and(query_param_is_missing("endRow"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resources": [],
            "totalCount": 0
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_list(
        &targets,
        "Hosts",
        "EC2_Instances",
        None,
        &[],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_list should succeed with an empty response");
}

#[tokio::test]
async fn list_rejects_invalid_scope_before_any_request() {
    let server = MockServer::start().await;
    // No mocks mounted: an invalid --scope must fail client-side without HTTP.

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let scope = vec!["region=us-east-1".to_string()];

    let result = run_list(
        &targets,
        "Hosts",
        "EC2_Instances",
        None,
        &scope,
        None,
        None,
        OutputFormat::Json,
    )
    .await;

    let err = result.expect_err("unknown scope key should error");
    assert!(err.to_string().contains("unknown --scope key 'region'"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn health_history_percent_encodes_resource_id() {
    let server = MockServer::start().await;

    // `:` and `=` in the resource id must reach the server percent-encoded.
    Mock::given(method("GET"))
        .and(path(format!(
            "{BASE}/1001234%3Ahost_id%3Di-abc123/health-history"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "healthHistory": [
                { "timestamp": "2026-07-01T00:00:00Z", "status": "Healthy" },
                { "timestamp": "2026-07-02T00:00:00Z", "status": "Critical" }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_health_history(&targets, "1001234:host_id=i-abc123", OutputFormat::Json)
        .await
        .expect("run_health_history should hit the encoded path");
}

#[tokio::test]
async fn health_history_handles_empty_history() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/plain-id/health-history")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "healthHistory": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_health_history(&targets, "plain-id", OutputFormat::Json)
        .await
        .expect("empty health history should succeed");
}

#[tokio::test]
async fn raw_data_returns_document() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!(
            "{BASE}/1001234%3Ahost_id%3Di-abc123/raw-data"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "rawData": { "host_id": "i-abc123", "tags": { "env": "prod" } }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_raw_data(&targets, "1001234:host_id=i-abc123", OutputFormat::Json)
        .await
        .expect("run_raw_data should succeed");
}

#[tokio::test]
async fn raw_data_handles_null_document() {
    let server = MockServer::start().await;

    // A 200 with null rawData means "cleanly missing" - not an error.
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/plain-id/raw-data")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "rawData": null })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_raw_data(&targets, "plain-id", OutputFormat::Json)
        .await
        .expect("null raw data should succeed");
}

#[tokio::test]
async fn api_error_body_surfaces_in_message() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/types")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "category is required and must be non-empty"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_types(&targets, OutputFormat::Json)
        .await
        .expect_err("400 from the only profile should error");
    // The service's {"error": ...} body must survive into the user-facing message.
    assert!(err.to_string().contains("see above for details"));
}

#[tokio::test]
async fn all_output_formats_render() {
    for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Agents] {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(format!("{BASE}/types")))
            .respond_with(ResponseTemplate::new(200).set_body_json(types_body()))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];

        run_types(&targets, format)
            .await
            .unwrap_or_else(|e| panic!("run_types should render {format:?}: {e:#}"));
    }
}
