#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::keypaths::api::{
    GetKeypathSamplesRequest, SearchKeypathsRequest, TimeRange,
};
use coralogix_cli::commands::keypaths::{self, keypath_filter, parse_datasets, parse_labels};
use coralogix_cli::config::OutputFormat;

fn range() -> TimeRange {
    TimeRange {
        start: "2026-11-06T10:00:00Z".into(),
        end: "2026-11-06T11:00:00Z".into(),
    }
}

fn search_request() -> SearchKeypathsRequest {
    SearchKeypathsRequest {
        datasets: parse_datasets(&["logs".into(), "default/spans".into()]).unwrap(),
        time_range: range(),
        keypath_filter: keypath_filter(
            vec![],
            vec!["$d.kubernetes".into(), "$d.k8s".into()],
            vec!["pod".into()],
        ),
        dimension_filter: parse_labels(&["subsystemName=api".into(), "subsystemName=web".into()])
            .unwrap(),
        limit: Some(10),
    }
}

fn samples_request() -> GetKeypathSamplesRequest {
    GetKeypathSamplesRequest {
        datasets: parse_datasets(&["logs".into()]).unwrap(),
        keypath: "$m.severity".into(),
        time_range: range(),
        dimension_filter: None,
        limit: None,
    }
}

#[tokio::test]
async fn search_sends_the_request_and_renders_keypaths() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/schema-store/keypaths/search"))
        .and(body_partial_json(json!({
            "datasets": [
                {"dataspace": "default", "dataset": "logs"},
                {"dataspace": "default", "dataset": "spans"}
            ],
            "keypathFilter": {"all": {"filters": [
                {"any": {"filters": [{"prefix": "$d.kubernetes"}, {"prefix": "$d.k8s"}]}},
                {"contains": "pod"}
            ]}},
            "dimensionFilter": {"any": {"filters": [
                {"match": {"keypath": "$l.subsystemName", "value": "api"}},
                {"match": {"keypath": "$l.subsystemName", "value": "web"}}
            ]}},
            "limit": 10
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keypaths": [
                {"keypath": "$d.k8s.pod_ip", "types": ["string"]},
                {"keypath": "$d.kubernetes.pod_name", "types": ["string"]}
            ]
        })))
        .expect(3)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    for output in [OutputFormat::Json, OutputFormat::Toon, OutputFormat::Text] {
        keypaths::run_search(&targets, &search_request(), output)
            .await
            .expect("run_search should succeed");
    }
}

#[tokio::test]
async fn search_handles_truncated_and_empty_responses() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/schema-store/keypaths/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"truncated": true})))
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    keypaths::run_search(&targets, &search_request(), OutputFormat::Text)
        .await
        .expect("an empty, truncated search is not an error");
}

#[tokio::test]
async fn search_surfaces_a_rejected_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/schema-store/keypaths/search"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "code": "InvalidArgument",
            "message": "validation error: limit 1001 exceeds the maximum of 1000"
        })))
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let error = keypaths::run_search(&targets, &search_request(), OutputFormat::Json)
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
            .and(path("/api/v1/schema-store/keypaths/search"))
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
    keypaths::run_search(&targets, &search_request(), OutputFormat::Json)
        .await
        .expect("multi-profile search should succeed");
}
