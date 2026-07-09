#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::ai_center::{
    run_add_policy, run_applications_get, run_applications_list, run_count,
    run_custom_evaluations_for_application, run_custom_evaluations_list, run_evaluations_delete,
    run_evaluations_list, run_model_pricing_get, run_model_pricing_set, run_remove_policy,
};
use coralogix_cli::config::OutputFormat;

const AI: &str = "/mgmt/openapi/5/ai";

#[tokio::test]
async fn applications_list_hits_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{AI}/applications/v3")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "aiApplications": [
                { "id": "a-1", "application": "Prod", "subsystem": "Advisor", "guardrailsIntegrated": true }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_applications_list(&targets, None, None, &[], OutputFormat::Json)
        .await
        .expect("applications list should succeed");
}

#[tokio::test]
async fn applications_list_empty_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{AI}/applications/v3")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_applications_list(&targets, None, None, &[], OutputFormat::Text)
        .await
        .expect("empty applications list should succeed");
}

#[tokio::test]
async fn applications_get_hits_id_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{AI}/applications/v3/app-123")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "app-123" })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_applications_get(&targets, "app-123", OutputFormat::Json)
        .await
        .expect("applications get should succeed");
}

#[tokio::test]
async fn evaluations_list_forwards_scope_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{AI}/evaluations/v3")))
        .and(query_param("application", "Prod"))
        .and(query_param("subsystem", "Advisor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "aiEvaluations": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_evaluations_list(
        &targets,
        Some("Prod"),
        Some("Advisor"),
        None,
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("evaluations list should succeed");
}

#[tokio::test]
async fn coverage_hits_per_type_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{AI}/evaluation-counts/v3/per-type")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "counts": { "toxicity": 3 } })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_count(&targets, OutputFormat::Json)
        .await
        .expect("count should succeed");
}

#[tokio::test]
async fn custom_evaluations_list_and_for_application() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{AI}/custom-evaluations/v3")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [ { "id": "c-1", "name": "No PII", "applicationIds": ["a-1"] } ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "{AI}/custom-evaluations/v3/by-application/app-9"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_custom_evaluations_list(&targets, OutputFormat::Json)
        .await
        .expect("custom-evaluations list should succeed");
    run_custom_evaluations_for_application(&targets, "app-9", OutputFormat::Json)
        .await
        .expect("custom-evaluations for-application should succeed");
}

#[tokio::test]
async fn model_pricing_get_and_set() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{AI}/model-pricing/v3")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "prices": {} })))
        .mount(&server)
        .await;
    // `set` reads the model→price map from stdin and must wrap it as {"prices": …}.
    Mock::given(method("PUT"))
        .and(path(format!("{AI}/model-pricing/v3")))
        .and(body_json(
            json!({ "prices": { "gpt-4o": { "inputPricePerMillionTokens": 2.5 } } }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_model_pricing_get(&targets, OutputFormat::Json)
        .await
        .expect("model-pricing get should succeed");

    let dir = std::env::temp_dir();
    let file = dir.join("cx_ai_center_pricing_test.json");
    std::fs::write(
        &file,
        json!({ "gpt-4o": { "inputPricePerMillionTokens": 2.5 } }).to_string(),
    )
    .unwrap();
    run_model_pricing_set(&targets, file.to_str().unwrap(), OutputFormat::Json)
        .await
        .expect("model-pricing set should succeed");
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn evaluations_delete_uses_delete_verb() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("{AI}/evaluations/v3/e-1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_evaluations_delete(&targets, "e-1", OutputFormat::Json)
        .await
        .expect("evaluations delete should succeed");
}

#[tokio::test]
async fn add_and_remove_policy_use_link_routes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "{AI}/custom-evaluations/v3/c-1/applications/a-1"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!(
            "{AI}/custom-evaluations/v3/c-1/applications/a-1"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_add_policy(&targets, "c-1", "a-1", OutputFormat::Json)
        .await
        .expect("add-policy should succeed");
    run_remove_policy(&targets, "c-1", "a-1", OutputFormat::Json)
        .await
        .expect("remove-policy should succeed");
}
