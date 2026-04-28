mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::tco_policies::{run_get, run_list, run_settings};
use cx::config::OutputFormat;

#[tokio::test]
async fn list_tco_policies_returns_items_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "policies": [
            {
                "id": "policy-001",
                "name": "Production Logs",
                "priority": "PRIORITY_TYPE_HIGH",
                "sourceType": "SOURCE_TYPE_LOGS",
                "severity": "SEVERITY_INFO",
                "enabled": true,
                "archiveRetention": {"id": "ret-001"}
            },
            {
                "id": "policy-002",
                "name": "Debug Spans",
                "priority": "PRIORITY_TYPE_LOW",
                "sourceType": "SOURCE_TYPE_SPANS"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataplans/policies/v1"))
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
async fn get_tco_policy_by_id() {
    let server = MockServer::start().await;

    let body = json!({
        "policy": {
            "id": "policy-001",
            "name": "Production Logs",
            "priority": "PRIORITY_TYPE_HIGH"
        }
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/dataplans/policies/v1/policy-001",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_get(&targets, "policy-001", OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}

#[tokio::test]
async fn get_tco_settings() {
    let server = MockServer::start().await;

    let body = json!({
        "enabled": true,
        "defaultPolicy": "archive"
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataplans/policy-settings/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_settings(&targets, OutputFormat::Json)
        .await
        .expect("run_settings should succeed");
}
