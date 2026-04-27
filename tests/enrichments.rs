mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::enrichments::{run_limit, run_list, run_settings};
use cx::config::OutputFormat;

#[tokio::test]
async fn list_enrichments_from_mock() {
    let server = MockServer::start().await;

    let body = json!([
        { "id": "enr-001", "fieldName": "coralogix.metadata.applicationName", "enrichmentType": "GEO_IP", "source": "ip_field" }
    ]);

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/enrichments/enrichments/v1"))
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
async fn list_enrichments_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/enrichments/enrichments/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list should succeed with empty response");
}

#[tokio::test]
async fn enrichment_limit_from_mock() {
    let server = MockServer::start().await;

    let body = json!({ "maxEnrichmentsPerAccount": 500 });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/enrichments/enrichments/v1/limit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_limit(&[target], OutputFormat::Json)
        .await
        .expect("run_limit should succeed");
}

#[tokio::test]
async fn enrichment_settings_from_mock() {
    let server = MockServer::start().await;

    let body = json!({ "enabled": true });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/enrichments/enrichments/v1/settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_settings(&[target], OutputFormat::Json)
        .await
        .expect("run_settings should succeed");
}
