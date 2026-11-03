#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::rum::api::StackFrame;
use coralogix_cli::commands::rum::{run_source_maps_list, run_stacktrace};
use coralogix_cli::config::OutputFormat;

const RELEASES_PATH: &str = "/mgmt/openapi/5/rum/source-maps/v1/releases";
const MAP_PATH: &str = "/mgmt/openapi/5/rum/source-maps/v1/stack-traces:map";

#[tokio::test]
async fn list_follows_page_tokens() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(RELEASES_PATH))
        .and(query_param("application", "shop"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "releases": [{ "application": "shop", "version": "2.0.0", "platform": "SOURCE_MAP_PLATFORM_WEB" }],
            "nextPageToken": "MQ"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(RELEASES_PATH))
        .and(query_param("pageToken", "MQ"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "releases": [{ "application": "shop", "version": "1.0.0", "platform": "SOURCE_MAP_PLATFORM_WEB" }],
            "nextPageToken": ""
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    for output in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Toon] {
        run_source_maps_list(&targets, Some("shop"), output)
            .await
            .expect("run_source_maps_list should succeed");
        server.verify().await;
        server.reset().await;
        Mock::given(method("GET"))
            .and(path(RELEASES_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "releases": [] })))
            .mount(&server)
            .await;
    }
}

#[tokio::test]
async fn stacktrace_sends_frames_and_renders_mapping() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(MAP_PATH))
        .and(body_json(json!({
            "application": "shop",
            "version": "1.2.3",
            "platform": "SOURCE_MAP_PLATFORM_REACT_NATIVE_ANDROID",
            "frames": [
                { "fileName": "index.android.bundle", "lineNumber": 1, "columnNumber": 0 },
                { "fileName": "vendor.js", "lineNumber": 3 }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "application": "shop",
            "version": "1.2.3",
            "commitHash": "abc123",
            "repoName": "org/shop",
            "frames": [
                {
                    "fileName": "index.android.bundle", "lineNumber": 1, "columnNumber": 0,
                    "status": "STACK_FRAME_MAPPING_STATUS_MAPPED",
                    "source": {
                        "fileName": "src/cart.ts", "lineNumber": 42, "columnNumber": 7,
                        "functionName": "addToCart",
                        "codeLines": [{ "lineNumber": 42, "text": "throw new Error('empty cart')" }]
                    }
                },
                { "fileName": "vendor.js", "lineNumber": 3, "status": "STACK_FRAME_MAPPING_STATUS_SOURCE_MAP_NOT_FOUND" }
            ]
        })))
        .expect(3)
        .mount(&server)
        .await;

    let frames = vec![
        StackFrame {
            file_name: "index.android.bundle".into(),
            line_number: 1,
            column_number: Some(0),
        },
        StackFrame {
            file_name: "vendor.js".into(),
            line_number: 3,
            column_number: None,
        },
    ];
    let targets = vec![common::test_target("test-profile", &server.uri())];
    for output in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Toon] {
        run_stacktrace(&targets, "shop", "1.2.3", "android", frames.clone(), output)
            .await
            .expect("run_stacktrace should succeed");
    }
}

#[tokio::test]
async fn stacktrace_surfaces_api_errors() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(MAP_PATH))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "code": 3,
            "message": "application shop version 1.2.3 has React Native source maps, not web ones."
        })))
        .mount(&server)
        .await;

    let frames = vec![StackFrame {
        file_name: "main.js".into(),
        line_number: 1,
        column_number: Some(1),
    }];
    let targets = vec![common::test_target("test-profile", &server.uri())];
    let error = run_stacktrace(&targets, "shop", "1.2.3", "web", frames, OutputFormat::Json)
        .await
        .expect_err("a 400 should fail the command");
    assert!(format!("{error:#}").contains("React Native"), "{error:#}");
}
