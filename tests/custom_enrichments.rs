mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::custom_enrichments::{run_get, run_list};
use cx::config::OutputFormat;

#[tokio::test]
async fn list_custom_enrichments_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "customEnrichments": [
            { "id": "ce-001", "name": "IP Lookup", "enrichmentType": "CUSTOM_ENRICHMENT_TYPE_GEO_IP" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/enrichments/custom-enrichments/v1"))
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
async fn list_custom_enrichments_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/enrichments/custom-enrichments/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"customEnrichments": []})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list should succeed with empty response");
}

#[tokio::test]
async fn get_custom_enrichment_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "customEnrichment": {
            "id": "ce-001",
            "name": "IP Lookup",
            "enrichmentType": "CUSTOM_ENRICHMENT_TYPE_GEO_IP"
        }
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/enrichments/custom-enrichments/v1/ce-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_get(&[target], "ce-001", OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}
