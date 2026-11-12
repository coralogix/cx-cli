use std::fs;
use std::path::PathBuf;

#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::integrations::{
    run_create, run_definition, run_delete, run_deployed, run_list, run_template, run_test,
    run_update,
};
use coralogix_cli::config::OutputFormat;

fn write_json_file(body: serde_json::Value) -> PathBuf {
    let path = std::env::temp_dir().join(format!("cx-integrations-test-{}.json", Uuid::new_v4()));
    fs::write(
        &path,
        serde_json::to_vec(&body).expect("JSON should serialize"),
    )
    .expect("test JSON should be written");
    path
}

#[tokio::test]
async fn list_integrations_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "integrations": [
            {
                "integration": {
                    "id": "aws-sns-shipper",
                    "name": "AWS SNS",
                    "tags": ["AWS", "Logs"],
                    "versions": ["0.0.40"],
                    "integrationType": { "cloudformation": {} }
                },
                "errors": []
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/integrations/v1"))
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
async fn list_integrations_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/integrations/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "integrations": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list should succeed with empty response");
}

#[tokio::test]
async fn update_uses_metadata_endpoint_and_normalizes_deployment_details() {
    let server = MockServer::start().await;
    let deployment_id = "deployment-123";
    let input = json!({
        "integrationDetail": {
            "integration": { "id": "aws-metrics-collector" },
            "default": {
                "registered": [{
                    "id": deployment_id,
                    "definitionVersion": "0.11.0",
                    "parameters": [{ "key": "ApplicationName", "stringValue": "test" }]
                }]
            }
        }
    });
    let expected = json!({
        "id": deployment_id,
        "metadata": {
            "integrationKey": "aws-metrics-collector",
            "integrationParameters": {
                "parameters": [{ "key": "ApplicationName", "stringValue": "test" }]
            },
            "version": "0.11.0"
        }
    });
    let file = write_json_file(input);

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/integrations/metadata/v1"))
        .and(body_json(&expected))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_update(
        &[target],
        deployment_id,
        file.to_str().expect("temporary path must be UTF-8"),
        OutputFormat::Json,
    )
    .await
    .expect("run_update should use the metadata endpoint");
    fs::remove_file(file).expect("temporary file should be removed");
}

#[tokio::test]
async fn test_uses_metadata_endpoint_and_normalizes_deployment_details() {
    let server = MockServer::start().await;
    let deployment_id = "deployment-123";
    let input = json!({
        "integrationDetail": {
            "integration": { "id": "aws-metrics-collector" },
            "default": {
                "registered": [{
                    "id": deployment_id,
                    "definitionVersion": "0.11.0",
                    "parameters": [{ "key": "ApplicationName", "stringValue": "test" }]
                }]
            }
        }
    });
    let expected = json!({
        "integrationId": deployment_id,
        "integrationData": {
            "integrationKey": "aws-metrics-collector",
            "integrationParameters": {
                "parameters": [{ "key": "ApplicationName", "stringValue": "test" }]
            },
            "version": "0.11.0"
        }
    });
    let file = write_json_file(input);

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/integrations/metadata/v1/test"))
        .and(body_json(&expected))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "result": { "success": {} } })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_test(
        &[target],
        Some(deployment_id),
        file.to_str().expect("temporary path must be UTF-8"),
        OutputFormat::Json,
    )
    .await
    .expect("run_test should use the metadata endpoint");
    fs::remove_file(file).expect("temporary file should be removed");
}

#[tokio::test]
async fn test_cli_id_overrides_embedded_request_id() {
    let server = MockServer::start().await;
    let input = json!({
        "integrationId": "deployment-a",
        "integrationData": {
            "integrationKey": "aws-metrics-collector",
            "integrationParameters": { "parameters": [] },
            "version": "0.11.0"
        }
    });
    let expected = json!({
        "integrationId": "deployment-b",
        "integrationData": {
            "integrationKey": "aws-metrics-collector",
            "integrationParameters": { "parameters": [] },
            "version": "0.11.0"
        }
    });
    let file = write_json_file(input);

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/integrations/metadata/v1/test"))
        .and(body_json(&expected))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "result": { "success": {} } })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_test(
        &[target],
        Some("deployment-b"),
        file.to_str().expect("temporary path must be UTF-8"),
        OutputFormat::Json,
    )
    .await
    .expect("run_test should use the explicitly supplied deployment ID");
    fs::remove_file(file).expect("temporary file should be removed");
}

#[tokio::test]
async fn create_posts_metadata_envelope_and_reads_integration_id() {
    let server = MockServer::start().await;

    // The flat input shape is normalized into the API's `metadata` envelope.
    let expected = json!({
        "metadata": {
            "integrationKey": "K8s-Collector",
            "version": "0.12.1",
            "integrationParameters": {
                "parameters": [{ "key": "CollectorName", "stringValue": "c" }]
            }
        }
    });
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/integrations/metadata/v1"))
        .and(body_json(expected))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "integrationId": "inst-1" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let file = write_json_file(json!({
        "integrationKey": "K8s-Collector",
        "version": "0.12.1",
        "parameters": [{ "key": "CollectorName", "stringValue": "c" }]
    }));
    let target = common::test_target("test-profile", &server.uri());
    run_create(&[target], file.to_str().unwrap(), OutputFormat::Json)
        .await
        .expect("create should succeed");
    let _ = fs::remove_file(file);
}

#[tokio::test]
async fn definition_deployed_delete_and_template_use_documented_routes() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/integrations/integrations/v1/definitions/K8s-Collector",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "integrationDefinition": { "key": "K8s-Collector", "revisions": [] }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/integrations/integrations/v1/deployed/inst-1",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "integration": {
                "id": "inst-1",
                "definitionKey": "K8s-Collector",
                "definitionVersion": "0.12.1",
                "parameters": []
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(
            "/mgmt/openapi/5/integrations/integrations/v1/instances/inst-1",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/template/v1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "templateUrl": "https://example.com/t.yaml" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_definition(
        std::slice::from_ref(&target),
        "K8s-Collector",
        OutputFormat::Json,
    )
    .await
    .expect("definition should succeed");
    run_deployed(std::slice::from_ref(&target), "inst-1", OutputFormat::Json)
        .await
        .expect("deployed should succeed");
    run_delete(std::slice::from_ref(&target), "inst-1")
        .await
        .expect("delete should succeed");
    run_template(&[target], OutputFormat::Json)
        .await
        .expect("template should succeed");
}
