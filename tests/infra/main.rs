#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::infra::{
    run_filters, run_health_history, run_list, run_raw_data, run_types,
};
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

fn list_body() -> serde_json::Value {
    json!({
        "resources": [
            {
                "resourceId": "4013226:host_id=i-077a1626590913a16",
                "name": "prod-api-01",
                "columns": { "Name": "prod-api-01", "Region": "eu-west-1" },
                "category": "Hosts",
                "type": "EC2_Instances"
            }
        ],
        "totalCount": 1
    })
}

fn filters_body() -> serde_json::Value {
    json!({
        "filters": [
            { "name": "Region", "kind": "string", "wildcard": true },
            {
                "name": "Health",
                "kind": "status",
                "wildcard": false,
                "values": ["critical", "healthy", "unmonitored"]
            }
        ]
    })
}

#[tokio::test]
async fn filters_sends_both_query_params() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/filters")))
        .and(query_param("category", "Hosts"))
        .and(query_param("type", "EC2_Instances"))
        .respond_with(ResponseTemplate::new(200).set_body_json(filters_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_filters(
        &targets,
        Some("Hosts"),
        Some("EC2_Instances"),
        OutputFormat::Json,
    )
    .await
    .expect("run_filters should send both query params");
}

#[tokio::test]
async fn filters_omits_absent_query_params() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/filters")))
        .and(query_param_is_missing("category"))
        .and(query_param_is_missing("type"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "filters": [
                {
                    "name": "Region",
                    "kind": "string",
                    "wildcard": true,
                    "types": [{ "category": "Hosts", "type": "EC2_Instances" }]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_filters(&targets, None, None, OutputFormat::Json)
        .await
        .expect("run_filters should omit both query params when unset");
}

#[tokio::test]
async fn filters_accepts_a_category_without_a_type() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/filters")))
        .and(query_param("category", "Hosts"))
        .and(query_param_is_missing("type"))
        .respond_with(ResponseTemplate::new(200).set_body_json(filters_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_filters(&targets, Some("Hosts"), None, OutputFormat::Json)
        .await
        .expect("run_filters should accept a category alone");
}

#[tokio::test]
async fn filters_renders_an_empty_list_as_text() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/filters")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "filters": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_filters(&targets, Some("Hosts"), None, OutputFormat::Text)
        .await
        .expect("run_filters should render an empty list without failing");
}

#[tokio::test]
async fn filters_merges_multiple_profiles() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    for server in [&server_a, &server_b] {
        Mock::given(method("GET"))
            .and(path(format!("{BASE}/filters")))
            .respond_with(ResponseTemplate::new(200).set_body_json(filters_body()))
            .expect(1)
            .mount(server)
            .await;
    }

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    run_filters(&targets, Some("Hosts"), None, OutputFormat::Json)
        .await
        .expect("run_filters should merge both profiles");
}

#[tokio::test]
async fn filters_rejects_a_blank_category_before_any_request() {
    let server = MockServer::start().await;
    // No mocks mounted: a blank --category must fail client-side without HTTP.

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_filters(&targets, Some("   "), None, OutputFormat::Json)
        .await
        .expect_err("a blank --category should be rejected");

    assert!(
        err.to_string().contains("--category"),
        "unexpected error: {err}"
    );
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
        Some("Hosts"),
        Some("EC2_Instances"),
        Some("web"),
        &scope,
        &[],
        &[],
        Some(100),
        Some(200),
        OutputFormat::Json,
    )
    .await
    .expect("run_list should send all query params");
}

#[tokio::test]
async fn match_all_flags_post_an_and_of_every_attribute() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(BASE))
        .and(body_json(json!({
            "category": "Hosts",
            "type": "EC2_Instances",
            "filter": {"bool": {"op": "AND", "operands": [
                {"match": {"field": "Region", "values": ["eu-west-1"]}},
                {"match": {"field": "Health", "values": ["critical"]}}
            ]}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_list(
        &targets,
        Some("Hosts"),
        Some("EC2_Instances"),
        None,
        &[],
        &["Region=eu-west-1".to_string(), "Health=critical".to_string()],
        &[],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_list should post an AND of both attributes");
}

#[tokio::test]
async fn match_any_flags_post_an_or_across_attributes() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(BASE))
        .and(body_json(json!({
            "filter": {"bool": {"op": "OR", "operands": [
                {"match": {"field": "Name", "values": ["coredns"]}},
                {"match": {"field": "Namespace", "values": ["kube-system"]}}
            ]}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_list(
        &targets,
        None,
        None,
        None,
        &[],
        &[],
        &["Name=coredns".to_string(), "Namespace=kube-system".to_string()],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_list should post an OR across the two attributes");
}

#[tokio::test]
async fn the_two_groups_and_with_each_other() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(BASE))
        .and(body_json(json!({
            "category": "Hosts",
            "filter": {"bool": {"op": "AND", "operands": [
                {"match": {"field": "OS", "values": ["Linux"]}},
                {"bool": {"op": "OR", "operands": [
                    {"match": {"field": "Health", "values": ["critical"]}},
                    {"match": {"field": "Region", "values": ["eu-west-1"]}}
                ]}}
            ]}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_list(
        &targets,
        Some("Hosts"),
        None,
        None,
        &[],
        &["OS=Linux".to_string()],
        &[
            "Health=critical".to_string(),
            "Region=eu-west-1".to_string(),
        ],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_list should nest the OR group inside the AND");
}

#[tokio::test]
async fn a_comma_ors_the_values_of_one_attribute() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(BASE))
        .and(body_json(json!({
            "filter": {"match": {"field": "Region", "values": ["eu-west-1", "us-east-1"]}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_list(
        &targets,
        None,
        None,
        None,
        &[],
        &["Region=eu-west-1,us-east-1".to_string()],
        &[],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_list should send one match carrying both values");
}

/// A single attribute needs no `bool` wrapper - the server accepts a bare node.
#[tokio::test]
async fn one_attribute_posts_a_bare_match() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(BASE))
        .and(body_json(json!({
            "filter": {"match": {"field": "Health", "values": ["critical"]}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_list(
        &targets,
        None,
        None,
        None,
        &[],
        &["Health=critical".to_string()],
        &[],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_list should post a bare match");
}

/// Paging stays on the query string even when the rest travels in the body.
#[tokio::test]
async fn a_filtered_request_keeps_paging_on_the_query_string() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(BASE))
        .and(query_param("startRow", "100"))
        .and(query_param("endRow", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_list(
        &targets,
        None,
        None,
        None,
        &[],
        &["Health=critical".to_string()],
        &[],
        Some(100),
        Some(200),
        OutputFormat::Json,
    )
    .await
    .expect("run_list should keep paging on the query string");
}

/// Scope filters stay on the query string whichever method is used - the server
/// reads them from the raw query map for both.
#[tokio::test]
async fn a_filtered_request_keeps_scope_filters_on_the_query_string() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(BASE))
        .and(query_param("scopeFilter.service", "checkout"))
        .and(body_json(json!({
            "filter": {"match": {"field": "Health", "values": ["critical"]}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_list(
        &targets,
        None,
        None,
        None,
        &["service=checkout".to_string()],
        &["Health=critical".to_string()],
        &[],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("scope filters belong on the query string on POST too");
}

/// `nameFilter` moves into the body, so it must not also appear on the query
/// string - the server refuses it there on POST.
#[tokio::test]
async fn a_filtered_request_sends_the_name_filter_in_the_body() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(BASE))
        .and(query_param_is_missing("nameFilter"))
        .and(query_param_is_missing("category"))
        .and(query_param_is_missing("type"))
        .and(body_json(json!({
            "category": "Hosts",
            "nameFilter": "web",
            "filter": {"match": {"field": "Health", "values": ["critical"]}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_list(
        &targets,
        Some("Hosts"),
        None,
        Some("web"),
        &[],
        &["Health=critical".to_string()],
        &[],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("a filtered request carries nameFilter in the body");
}

/// `fan_out` runs one future per profile concurrently and the filter is shared
/// across them by reference, unlike every other captured parameter.
#[tokio::test]
async fn a_filtered_list_fans_out_across_profiles() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    for server in [&server_a, &server_b] {
        Mock::given(method("POST"))
            .and(path(BASE))
            .and(body_json(json!({
                "filter": {"bool": {"op": "OR", "operands": [
                    {"match": {"field": "Name", "values": ["coredns"]}},
                    {"match": {"field": "Namespace", "values": ["kube-system"]}}
                ]}}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(list_body()))
            .expect(1)
            .mount(server)
            .await;
    }

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    run_list(
        &targets,
        None,
        None,
        None,
        &[],
        &[],
        &["Name=coredns".to_string(), "Namespace=kube-system".to_string()],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("a filtered list should send the same filter to both profiles");
}

#[tokio::test]
async fn a_repeated_attribute_in_one_group_is_refused_before_any_request() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_list(
        &targets,
        None,
        None,
        None,
        &[],
        &["Region=eu-west-1".to_string(), "Region=us-east-1".to_string()],
        &[],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect_err("a repeated attribute must be refused");

    let msg = err.to_string();
    assert!(msg.contains("'Region' given more than once"), "got: {msg}");
    assert!(msg.contains("Region=a,b"), "got: {msg}");
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "nothing should reach the API"
    );
}

#[tokio::test]
async fn a_filter_flag_without_an_equals_is_refused() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_list(
        &targets,
        None,
        None,
        None,
        &[],
        &["Region".to_string()],
        &[],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect_err("a flag without = must be refused");

    assert!(err.to_string().contains("expected NAME=VALUE"));
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
        Some("Hosts"),
        Some("EC2_Instances"),
        None,
        &[],
        &[],
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
        Some("Hosts"),
        Some("EC2_Instances"),
        None,
        &scope,
        &[],
        &[],
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
    // With a single profile the actual error propagates (FORGE-482), so the
    // service's {"error": ...} body must survive into the error chain.
    let chain = format!("{err:#}");
    assert!(chain.contains("category is required and must be non-empty"));
    assert!(chain.contains("profile 'test-profile' failed"));
}

#[tokio::test]
async fn all_output_formats_render() {
    for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
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

// ── Single-profile enforcement for the id-taking subcommands ──────────────────

/// A resource id embeds the team id, so it can only resolve in the profile it
/// came from. The command must refuse before issuing any request.
#[tokio::test]
async fn health_history_rejects_multiple_profiles() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "healthHistory": [] })))
        .expect(0)
        .mount(&server)
        .await;

    let targets = vec![
        common::test_target("prod", &server.uri()),
        common::test_target("staging", &server.uri()),
    ];

    let err = run_health_history(&targets, "1001234:host_id=i-abc123", OutputFormat::Json)
        .await
        .expect_err("health-history must reject multiple profiles");
    let msg = format!("{err:#}");
    assert!(msg.contains("health-history"), "got: {msg}");
    assert!(msg.contains("single profile"), "got: {msg}");
    assert!(
        msg.contains('2'),
        "error should name the profile count, got: {msg}"
    );
}

#[tokio::test]
async fn raw_data_rejects_multiple_profiles() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "rawData": null })))
        .expect(0)
        .mount(&server)
        .await;

    let targets = vec![
        common::test_target("prod", &server.uri()),
        common::test_target("staging", &server.uri()),
    ];

    let err = run_raw_data(&targets, "1001234:host_id=i-abc123", OutputFormat::Json)
        .await
        .expect_err("raw-data must reject multiple profiles");
    let msg = format!("{err:#}");
    assert!(msg.contains("raw-data"), "got: {msg}");
    assert!(msg.contains("single profile"), "got: {msg}");
}

/// `types` and `list` take no resource id, so comparing fleets across accounts
/// is a supported use - the guard must not leak into them.
#[tokio::test]
async fn types_and_list_still_fan_out_across_profiles() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/types")))
        .respond_with(ResponseTemplate::new(200).set_body_json(types_body()))
        .expect(2)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path(BASE))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resources": [{ "resourceId": "1001234:host_id=i-abc123", "name": "web-1" }],
            "totalCount": 1
        })))
        .expect(2)
        .mount(&server)
        .await;

    let targets = vec![
        common::test_target("prod", &server.uri()),
        common::test_target("staging", &server.uri()),
    ];

    run_types(&targets, OutputFormat::Json)
        .await
        .expect("types should fan out");
    run_list(
        &targets,
        Some("Hosts"),
        Some("EC2_Instances"),
        None,
        &[],
        &[],
        &[],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("list should fan out");
}
