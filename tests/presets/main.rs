#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::presets::{run_create, run_list, run_set_default, run_update};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_presets_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "presetSummaries": [
            { "id": "preset-001", "name": "Default Slack", "connectorType": "SLACK", "presetType": "SYSTEM" }
        ]
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/presets:summariesList",
        ))
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
async fn create_preset_from_mock() {
    let server = MockServer::start().await;

    let input_body = json!({
        "name": "Custom Slack",
        "parentId": "preset_system_slack_alerts_basic",
        "description": "Custom preset for alerts",
        "attachmentConfig": {},
        "configOverrides": []
    });
    let expected_request = json!({ "preset": input_body.clone() });
    let response_body = json!({
        "preset": {
            "id": "preset-custom-001",
            "name": "Custom Slack",
            "parentId": "preset_system_slack_alerts_basic",
            "description": "Custom preset for alerts",
            "attachmentConfig": {},
            "configOverrides": []
        }
    });

    Mock::given(method("POST"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/presets:createCustom",
        ))
        .and(body_json(expected_request))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&server)
        .await;

    let path = std::env::temp_dir().join(format!(
        "cx-preset-create-{}-{}.json",
        std::process::id(),
        line!()
    ));
    std::fs::write(&path, input_body.to_string()).expect("write preset fixture");

    let target = common::test_target("test-profile", &server.uri());
    let result = run_create(&[target], path.to_str().unwrap(), OutputFormat::Json).await;

    std::fs::remove_file(path).expect("remove preset fixture");
    result.expect("run_create should succeed");
}

#[tokio::test]
async fn update_preset_from_mock() {
    let server = MockServer::start().await;

    let input_body = json!({
        "id": "preset-custom-001",
        "name": "Custom Slack Updated",
        "parentId": "preset_system_slack_alerts_basic",
        "description": "Updated custom preset for alerts",
        "attachmentConfig": {},
        "configOverrides": []
    });
    let expected_request = json!({ "preset": input_body.clone() });
    let response_body = json!({
        "preset": {
            "id": "preset-custom-001",
            "name": "Custom Slack Updated",
            "parentId": "preset_system_slack_alerts_basic",
            "description": "Updated custom preset for alerts",
            "attachmentConfig": {},
            "configOverrides": []
        }
    });

    Mock::given(method("PUT"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/presets:replaceCustom",
        ))
        .and(body_json(expected_request))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&server)
        .await;

    let path = std::env::temp_dir().join(format!(
        "cx-preset-update-{}-{}.json",
        std::process::id(),
        line!()
    ));
    std::fs::write(&path, input_body.to_string()).expect("write preset fixture");

    let target = common::test_target("test-profile", &server.uri());
    let result = run_update(&[target], path.to_str().unwrap(), OutputFormat::Json).await;

    std::fs::remove_file(path).expect("remove preset fixture");
    result.expect("run_update should succeed");
}

#[tokio::test]
async fn set_default_preset_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/presets/preset-system-001/default/apply",
        ))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_set_default(&[target], "preset-system-001")
        .await
        .expect("run_set_default should succeed");
}
