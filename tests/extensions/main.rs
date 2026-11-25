use std::fs;
use std::path::PathBuf;

#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::extensions::{
    run_deploy, run_deployed, run_list, run_undeploy, run_update,
};
use coralogix_cli::config::OutputFormat;

fn write_json_file(body: serde_json::Value) -> PathBuf {
    let path = std::env::temp_dir().join(format!("cx-extensions-test-{}.json", Uuid::new_v4()));
    fs::write(
        &path,
        serde_json::to_vec(&body).expect("JSON should serialize"),
    )
    .expect("test JSON should be written");
    path
}

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

#[tokio::test]
async fn deploy_update_and_undeploy_use_the_deployed_route() {
    let server = MockServer::start().await;

    // Deploy and update carry the deployment as query parameters (the
    // gateway ignores a JSON body on these routes); undeploy sends a body.
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/integrations/extensions/v1/deployed"))
        .and(query_param("id", "ChecklyObservability"))
        .and(query_param("version", "0.1.0"))
        .and(query_param("item_ids", "item-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "extensionDeployment": { "id": "ChecklyObservability", "version": "0.1.0" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/integrations/extensions/v1/deployed"))
        .and(query_param("id", "ChecklyObservability"))
        .and(query_param("applications", "prod"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "extensionDeployment": { "id": "ChecklyObservability", "applications": ["prod"] }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/mgmt/openapi/5/integrations/extensions/v1/deployed"))
        .and(body_json(
            json!({ "id": "ChecklyObservability", "keptExtensionItems": [] }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "failedItems": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let deploy = write_json_file(json!({
        "id": "ChecklyObservability", "version": "0.1.0", "itemIds": ["item-1"]
    }));
    let update = write_json_file(json!({
        "id": "ChecklyObservability", "version": "0.1.0", "itemIds": ["item-1"],
        "applications": ["prod"]
    }));
    let undeploy = write_json_file(json!({
        "id": "ChecklyObservability", "keptExtensionItems": []
    }));

    let target = common::test_target("test-profile", &server.uri());
    run_deploy(
        std::slice::from_ref(&target),
        deploy.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await
    .expect("deploy should succeed");
    run_update(
        std::slice::from_ref(&target),
        update.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await
    .expect("update should succeed");
    run_undeploy(&[target], undeploy.to_str().unwrap(), OutputFormat::Json)
        .await
        .expect("undeploy should succeed");

    for f in [deploy, update, undeploy] {
        let _ = fs::remove_file(f);
    }
}
