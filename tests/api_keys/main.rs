#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::api_keys::{run_list, run_send_data_keys};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_api_keys_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "keys": [
            { "id": "key-001", "name": "My Key", "owner": { "userId": "u1" }, "active": true, "hashed": true }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/api-keys/v3/list/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    // Mirror the real API: any single-segment suffix is parsed as a key ID, so a
    // regression back to `/list` gets the 400 users saw instead of a bare 404.
    Mock::given(method("GET"))
        .and(path_regex(r"^/mgmt/openapi/5/aaa/api-keys/v3/[^/]+$"))
        .respond_with(ResponseTemplate::new(400).set_body_json(
            json!({ "code": 400, "message": "Bad Request: key_id invalid character" }),
        ))
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
