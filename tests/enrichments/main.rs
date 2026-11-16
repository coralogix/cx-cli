#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::enrichments::{run_limit, run_list, run_settings};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_enrichments_from_mock() {
    let server = MockServer::start().await;

    let body = json!([
        { "id": "enr-001", "fieldName": "coralogix.metadata.applicationName", "enrichmentType": "GEO_IP", "source": "ip_field" }
    ]);

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/enrichment-rules/enrichment-rules/v1"))
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
        .and(path("/mgmt/openapi/5/enrichment-rules/enrichment-rules/v1"))
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
        .and(path(
            "/mgmt/openapi/5/enrichment-rules/enrichment-rules/v1/limit",
        ))
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
        .and(path(
            "/mgmt/openapi/5/enrichment-rules/enrichment-rules/v1/settings",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_settings(&[target], OutputFormat::Json)
        .await
        .expect("run_settings should succeed");
}

// ── remove / overwrite against the real v1 wire shapes ──────────────────────

fn temp_json(label: &str, body: &serde_json::Value) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "cx-enrichments-{label}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, serde_json::to_vec(body).unwrap()).unwrap();
    path
}

/// `DELETE /v1` takes the ids as repeated `enrichment_ids` query parameters;
/// a JSON body is ignored by the API.
#[tokio::test]
async fn remove_sends_ids_as_query_params() {
    use wiremock::matchers::query_param;

    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/mgmt/openapi/5/enrichment-rules/enrichment-rules/v1"))
        .and(query_param("enrichment_ids", "272627"))
        .and(query_param("enrichment_ids", "272628"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "remainingEnrichments": [] })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let file = temp_json("rm", &json!({ "enrichmentIds": [272627, "272628"] }));
    let target = common::test_target("test-profile", &server.uri());
    coralogix_cli::commands::enrichments::run_remove(
        &[target],
        file.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await
    .expect("run_remove should succeed");
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn remove_rejects_body_without_ids() {
    let server = MockServer::start().await;
    // No mock: the CLI must fail before any request.
    let file = temp_json("rm-bad", &json!({ "requestEnrichments": [] }));
    let target = common::test_target("test-profile", &server.uri());
    let err = coralogix_cli::commands::enrichments::run_remove(
        &[target],
        file.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("enrichmentIds"), "{err}");
    let _ = std::fs::remove_file(&file);
}

/// `overwrite` is the atomic overwrite for all enrichment types at
/// `PUT /v1/all`; `PUT /v1` is the deprecated custom-only variant.
#[tokio::test]
async fn overwrite_puts_to_all() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/enrichment-rules/enrichment-rules/v1/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "enrichments": [
                { "id": 272629, "fieldName": "sourceIPs", "enrichmentType": { "geoIp": { "withAsn": true } } }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file = temp_json(
        "ow",
        &json!({
            "requestEnrichments": [
                { "fieldName": "sourceIPs", "enrichmentType": { "geoIp": { "withAsn": true } } }
            ]
        }),
    );
    let target = common::test_target("test-profile", &server.uri());
    coralogix_cli::commands::enrichments::run_overwrite(
        &[target],
        file.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await
    .expect("run_overwrite should succeed");
    let _ = std::fs::remove_file(&file);
}
