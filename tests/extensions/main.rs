#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::extensions::{run_deployed, run_list};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_extensions_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "extensions": [
            { "id": "ext-001", "name": "AWS CloudWatch", "version": "1.0.0", "deployed": false }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/integrations/extensions/v1"))
        .and(body_json(json!({})))
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
async fn list_deployed_extensions_from_mock() {
    let server = MockServer::start().await;

    // Realistic deployed payload: no name/deployed/updated fields, unlike
    // the catalog list response.
    let deployed_body = json!({
        "deployedExtensions": [
            {
                "id": "K8sObservability",
                "version": "1.0.3",
                "applications": ["prod-eu"],
                "subsystems": ["kube-system"],
                "itemIds": ["alert-1", "dash-1"],
                "summary": { "deployedItemCounts": { "alerts": 1, "grafanaDashboards": 1 } }
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/extensions/v1/deployed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&deployed_body))
        .expect(1)
        .mount(&server)
        .await;

    // The handler resolves names from the catalog, best-effort.
    let catalog_body = json!({
        "extensions": [
            { "id": "K8sObservability", "name": "Kubernetes Observability", "version": "1.0.3" }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/integrations/extensions/v1"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&catalog_body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_deployed(&[target], OutputFormat::Json)
        .await
        .expect("run_deployed should succeed");
}

#[tokio::test]
async fn list_deployed_extensions_survives_catalog_failure() {
    let server = MockServer::start().await;

    let deployed_body = json!({
        "deployedExtensions": [
            { "id": "CoralogixSystem", "version": "0.2.1" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/integrations/extensions/v1/deployed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&deployed_body))
        .expect(1)
        .mount(&server)
        .await;

    // No catalog mock mounted: the name lookup fails, but the command
    // must still succeed and render the deployed extensions.
    let target = common::test_target("test-profile", &server.uri());
    run_deployed(&[target], OutputFormat::Text)
        .await
        .expect("run_deployed should succeed without the catalog");
}
