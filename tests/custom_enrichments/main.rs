#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::custom_enrichments::{run_get, run_list};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_custom_enrichments_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "customEnrichments": [
            { "id": "ce-001", "name": "IP Lookup", "enrichmentType": "CUSTOM_ENRICHMENT_TYPE_GEO_IP" }
        ]
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/enrichment-rules/custom-enrichment-rules/v1",
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
async fn list_custom_enrichments_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/enrichment-rules/custom-enrichment-rules/v1",
        ))
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
        .and(path(
            "/mgmt/openapi/5/enrichment-rules/custom-enrichment-rules/v1/ce-001",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_get(&[target], "ce-001", OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}

/// The only search endpoint is `POST /v1/all/contents`, which answers every
/// table's full content; the CLI filters by table id and query text.
#[tokio::test]
async fn search_filters_all_contents_by_table_and_query() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/mgmt/openapi/5/enrichment-rules/custom-enrichment-rules/v1/all/contents",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "customEnrichmentsData": [
                {
                    "definition": { "id": 172830, "name": "ip_lookup", "version": 1 },
                    "textual": "ip,city\n1.2.3.4,London\n5.6.7.8,Paris"
                },
                {
                    "definition": { "id": 999, "name": "other" },
                    "textual": "k,v\nLondon,other-table"
                }
            ]
        })))
        .expect(2)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    coralogix_cli::commands::custom_enrichments::run_search(
        std::slice::from_ref(&target),
        "172830",
        "london",
        OutputFormat::Json,
    )
    .await
    .expect("run_search should succeed");

    // An id that no table has is an error, not an empty result.
    let err = coralogix_cli::commands::custom_enrichments::run_search(
        &[target],
        "1",
        "london",
        OutputFormat::Json,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("not found"), "{err}");
}
