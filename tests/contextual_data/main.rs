use std::fs;
use std::path::PathBuf;

#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::contextual_data::{run_get, run_list, run_update};
use coralogix_cli::config::OutputFormat;

fn write_json_file(body: serde_json::Value) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("cx-contextual-data-test-{}.json", Uuid::new_v4()));
    fs::write(
        &path,
        serde_json::to_vec(&body).expect("JSON should serialize"),
    )
    .expect("test JSON should be written");
    path
}

#[tokio::test]
async fn list_contextual_data_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "integrations": [
            {
                "integration": {
                    "id": "cd-001",
                    "name": "GitHub Commits",
                    "description": "Enrich logs with GitHub commit data"
                },
                "amountIntegrations": 2,
                "isDeprecated": false,
                "isNew": true,
                "upgradeAvailable": false,
                "errors": []
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/contextual-data/v1"))
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
async fn list_contextual_data_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/contextual-data/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "integrations": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list should succeed with empty");
}

#[tokio::test]
async fn update_puts_integration_id_and_metadata_on_the_collection_route() {
    let server = MockServer::start().await;

    let metadata = json!({
        "integrationKey": "StatusPage-Tracker",
        "version": "0.0.1",
        "integrationParameters": {
            "parameters": [{ "key": "IntegrationName", "stringValue": "n" }]
        }
    });
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/integrations/contextual-data/v1"))
        .and(body_json(
            json!({ "integrationId": "inst-1", "metadata": metadata }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    // Flat input shape (what `create` users already write) is accepted too.
    let file = write_json_file(json!({
        "integrationKey": "StatusPage-Tracker",
        "version": "0.0.1",
        "parameters": [{ "key": "IntegrationName", "stringValue": "n" }]
    }));
    let target = common::test_target("test-profile", &server.uri());
    run_update(
        &[target],
        "inst-1",
        file.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await
    .expect("update should succeed");
    let _ = fs::remove_file(file);
}

#[tokio::test]
async fn get_keeps_registered_instances() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/integrations/contextual-data/v1/StatusPage-Tracker",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "integrationDetail": {
                "integration": { "id": "StatusPage-Tracker", "name": "Statuspage Tracker" },
                "default": { "registered": [{ "id": "inst-1", "definitionVersion": "0.0.1" }] }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_get(&[target], "StatusPage-Tracker", OutputFormat::Json)
        .await
        .expect("get should succeed");
}
