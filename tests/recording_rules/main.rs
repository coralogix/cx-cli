#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::recording_rules::{run_get, run_list};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_recording_rules_returns_items_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "groups": [
            {
                "id": "rr-001",
                "name": "Error Rate Rules",
                "interval": "60s",
                "rules": [
                    {"record": "job:http_errors:rate5m", "expr": "rate(http_errors_total[5m])"}
                ],
                "createdAt": "2024-01-01T00:00:00Z"
            },
            {
                "id": "rr-002",
                "name": "Latency Rules",
                "interval": "30s",
                "rules": []
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/recording-rules/recording-rules/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_list(&targets, OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}

#[tokio::test]
async fn get_recording_rule_by_id() {
    let server = MockServer::start().await;

    let body = json!({
        "group": {
            "id": "rr-001",
            "name": "Error Rate Rules",
            "interval": "60s",
            "rules": [
                {"record": "job:http_errors:rate5m", "expr": "rate(http_errors_total[5m])"}
            ]
        }
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/recording-rules/recording-rules/v1/rr-001",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_get(&targets, "rr-001", OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}

// ── real v1 wire shapes ──────────────────────────────────────────────────────

/// `GET /v1` answers `{"sets": [{id, name, groups: [{interval: 60, rules}]}]}`.
#[tokio::test]
async fn list_recording_rules_sets_shape() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/recording-rules/recording-rules/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sets": [{
                "id": "01M74QGJWQMN8V73EZV31FKADD",
                "name": "my-set",
                "groups": [{
                    "id": "01M74QGJWR81HAA2WBJTVRW8FC",
                    "name": "my-set",
                    "interval": 60,
                    "limit": "0",
                    "rules": [{ "record": "a", "expr": "vector(1)", "labels": {} }],
                    "version": 0
                }]
            }]
        })))
        .expect(2)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(std::slice::from_ref(&target), OutputFormat::Json)
        .await
        .expect("run_list should succeed");
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list text should succeed");
}

/// `POST /v1` answers `{"id"}` only; the CLI reads the set back so it can
/// print the name and rule count.
#[tokio::test]
async fn create_recording_rules_reads_back_the_set() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/recording-rules/recording-rules/v1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "id": "01M74QHSJFM03JC96ANND27MCT" })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/recording-rules/recording-rules/v1/01M74QHSJFM03JC96ANND27MCT",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "01M74QHSJFM03JC96ANND27MCT",
            "name": "my-set",
            "groups": [{ "name": "my-set", "interval": 60, "rules": [{ "record": "a", "expr": "vector(1)" }] }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file = std::env::temp_dir().join(format!(
        "cx-recording-rules-create-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({
            "name": "my-set",
            "groups": [{ "name": "my-set", "interval": 60, "rules": [{ "record": "a", "expr": "vector(1)" }] }]
        }))
        .unwrap(),
    )
    .unwrap();

    let target = common::test_target("test-profile", &server.uri());
    coralogix_cli::commands::recording_rules::run_create(
        &[target],
        file.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await
    .expect("run_create should succeed");
    let _ = std::fs::remove_file(&file);
}

/// `PUT /v1/{id}` answers `{}`; the CLI reads the set back.
#[tokio::test]
async fn update_recording_rules_reads_back_the_set() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path(
            "/mgmt/openapi/5/recording-rules/recording-rules/v1/rr-001",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/recording-rules/recording-rules/v1/rr-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "rr-001",
            "name": "my-set",
            "groups": [{ "name": "my-set", "interval": 60, "rules": [{ "record": "a", "expr": "vector(2)" }] }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file = std::env::temp_dir().join(format!(
        "cx-recording-rules-update-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({ "groups": [{ "name": "my-set", "rules": [] }] })).unwrap(),
    )
    .unwrap();

    let target = common::test_target("test-profile", &server.uri());
    coralogix_cli::commands::recording_rules::run_update(
        &[target],
        "rr-001",
        file.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await
    .expect("run_update should succeed");
    let _ = std::fs::remove_file(&file);
}
