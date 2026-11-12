#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::actions::{run_list, run_update};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_actions_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "actions": [
            { "id": "act-001", "name": "Slack Alert", "type": "slack", "url": "https://hooks.slack.com/...", "isActive": true }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/actions/actions/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}

#[tokio::test]
async fn update_action_puts_collection_with_action_envelope() {
    // The live API answers `PUT /actions/actions/v2/{id}` with 405; the
    // replace is `PUT /actions/actions/v2` with `{"action": {...}}`. A bare
    // action in the file is wrapped.
    let server = MockServer::start().await;

    let action = json!({
        "id": "act-001", "name": "Renamed", "url": "https://example.com/x",
        "isPrivate": false, "isHidden": false, "sourceType": "SOURCE_TYPE_LOG"
    });

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/actions/actions/v2"))
        .and(body_json(json!({ "action": action })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "action": action })))
        .expect(1)
        .mount(&server)
        .await;

    let file_path = std::env::temp_dir().join("cx_test_actions_update.json");
    std::fs::write(&file_path, serde_json::to_vec(&action).unwrap()).unwrap();

    let target = common::test_target("test-profile", &server.uri());
    let result = run_update(&[target], file_path.to_str().unwrap(), OutputFormat::Json).await;

    let _ = std::fs::remove_file(&file_path);
    result.expect("run_update should PUT the collection with an action envelope");
}
