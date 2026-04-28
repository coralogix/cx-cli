#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::saml::{run_get, run_sp_params};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn get_saml_config_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "enabled": true,
        "idpUrl": "https://idp.example.com/saml",
        "certificate": "MIIC..."
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/team-saml/v1/configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_get(&[target], OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}

#[tokio::test]
async fn get_sp_params_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "spUrl": "https://app.coralogix.com/saml/consume",
        "spEntityId": "coralogix"
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/team-saml/v1/sp_parameters"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_sp_params(&[target], OutputFormat::Json)
        .await
        .expect("run_sp_params should succeed");
}
