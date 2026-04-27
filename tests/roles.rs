mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::roles::{run_list, run_system};
use cx::config::OutputFormat;

#[tokio::test]
async fn list_custom_roles_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "roles": [
            { "roleId": "role-001", "name": "Admin", "description": "Full access" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/custom-roles/v1"))
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
async fn list_system_roles_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "roles": [
            { "roleId": "sys-001", "name": "ReadOnly", "description": "Read-only access" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/system-roles/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_system(&[target], OutputFormat::Json)
        .await
        .expect("run_system should succeed");
}
