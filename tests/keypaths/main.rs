#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::keypaths::api::{
    DimensionMatch, GetKeypathSamplesRequest, KeypathFilter, KeypathMatchMode, KeypathScope,
    ListKeypathsRequest, TimeRange,
};
use coralogix_cli::commands::keypaths::{self, parse_datasets};
use coralogix_cli::config::OutputFormat;

fn range() -> TimeRange {
    TimeRange {
        start: "2026-11-06T10:00:00Z".into(),
        end: "2026-11-06T11:00:00Z".into(),
    }
}

fn list_request() -> ListKeypathsRequest {
    ListKeypathsRequest {
        datasets: parse_datasets(&["logs".into(), "default/spans".into()]).unwrap(),
        time_range: range(),
        keypath_filter: Some(KeypathFilter {
            value: "$d.kubernetes".into(),
            match_mode: KeypathMatchMode::StartsWith,
        }),
        scopes: vec![KeypathScope::UserData],
        dimension_filter: vec![DimensionMatch {
            keypath: "$l.subsystemName".into(),
            values: vec!["api".into(), "web".into()],
        }],
        limit: Some(10),
    }
}

fn samples_request() -> GetKeypathSamplesRequest {
    GetKeypathSamplesRequest {
        datasets: parse_datasets(&["logs".into()]).unwrap(),
        keypath: "$m.severity".into(),
        time_range: range(),
        dimension_filter: vec![],
        limit: None,
    }
}

#[tokio::test]
async fn list_sends_the_request_and_renders_keypaths() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/schema-store/keypaths/list"))
        .and(body_partial_json(json!({
            "datasets": [
                {"dataspace": "default", "dataset": "logs"},
                {"dataspace": "default", "dataset": "spans"}
            ],
            "keypathFilter": {"value": "$d.kubernetes", "matchMode": "KEYPATH_MATCH_MODE_STARTS_WITH"},
            "scopes": ["KEYPATH_SCOPE_USER_DATA"],
            "dimensionFilter": [{"keypath": "$l.subsystemName", "values": ["api", "web"]}],
            "limit": 10
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keypaths": [
                {"keypath": "$d.kubernetes.pod_name", "types": ["string"]},
                {"keypath": "$d.kubernetes.restarts", "types": ["number"]}
            ]
        })))
        .expect(3)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    for output in [OutputFormat::Json, OutputFormat::Toon, OutputFormat::Text] {
        keypaths::run_list(&targets, &list_request(), output)
            .await
            .expect("run_list should succeed");
    }
}

#[tokio::test]
async fn list_handles_truncated_and_empty_responses() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/schema-store/keypaths/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"truncated": true})))
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    keypaths::run_list(&targets, &list_request(), OutputFormat::Text)
        .await
        .expect("an empty, truncated listing is not an error");
}

#[tokio::test]
async fn list_surfaces_a_rejected_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/schema-store/keypaths/list"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "code": "InvalidArgument",
            "message": "validation error: limit 1001 exceeds the maximum of 1000"
        })))
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let error = keypaths::run_list(&targets, &list_request(), OutputFormat::Json)
        .await
        .expect_err("a 400 must fail the command");
    assert!(format!("{error:#}").contains("1000"), "{error:#}");
}

#[tokio::test]
async fn samples_sends_the_keypath_and_renders_samples() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/schema-store/keypaths/samples"))
        .and(body_partial_json(json!({
            "datasets": [{"dataspace": "default", "dataset": "logs"}],
            "keypath": "$m.severity"
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"samples": ["ERROR", "INFO"]})),
        )
        .expect(3)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    for output in [OutputFormat::Json, OutputFormat::Toon, OutputFormat::Text] {
        keypaths::run_samples(&targets, &samples_request(), output)
            .await
            .expect("run_samples should succeed");
    }
}

#[tokio::test]
async fn samples_handles_an_empty_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/schema-store/keypaths/samples"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    keypaths::run_samples(&targets, &samples_request(), OutputFormat::Text)
        .await
        .expect("no samples is not an error");
}

#[tokio::test]
async fn fan_out_merges_profiles() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    for (server, keypath) in [(&first, "$d.a"), (&second, "$d.b")] {
        Mock::given(method("POST"))
            .and(path("/api/v1/schema-store/keypaths/list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "keypaths": [{"keypath": keypath, "types": ["string"]}]
            })))
            .expect(1)
            .mount(server)
            .await;
    }

    let targets = vec![
        common::test_target("first", &first.uri()),
        common::test_target("second", &second.uri()),
    ];
    keypaths::run_list(&targets, &list_request(), OutputFormat::Json)
        .await
        .expect("multi-profile listing should succeed");
}
