#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::api_keys::{run_list, run_send_data_keys};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_api_keys_from_mock() {
    let server = MockServer::start().await;

    // The bare collection endpoint - there is no "list" route on the real
    // backend (see ApiKeysApi::list's doc comment) - returns every team
    // member's keys, matching what the Coralogix console's API Keys page
    // shows.
    let body = json!({
        "keys": [
            {
                "apiKey": {
                    "id": "key-001",
                    "keyName": "My Key",
                    "owner": { "user": { "userId": "u1", "userEmail": "me@example.com" } },
                    "isActive": true
                },
                "permissions": [],
                "presets": []
            },
            {
                "apiKey": {
                    "id": "key-002",
                    "keyName": "Someone Else's Key",
                    "owner": { "user": { "userId": "u2", "userEmail": "other@example.com" } },
                    "isActive": true
                },
                "permissions": [],
                "presets": []
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/api-keys/v3"))
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
async fn list_send_data_keys_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/send-data-keys/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"keys": []})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_send_data_keys(&[target], OutputFormat::Json)
        .await
        .expect("run_send_data_keys should succeed");
}
