#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::routers::run_list;
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_routers_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "routers": [
            { "id": "router-001", "name": "Alert Router", "entityType": "ENTITY_TYPE_ALERTS", "destinations": [{"id": "d1"}] }
        ]
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/routers",
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
