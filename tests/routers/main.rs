#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::routers::{run_list, run_validate_matcher};
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

#[tokio::test]
async fn validate_matcher_sends_labels_as_query_params() {
    // The live API ignores the request body and reads the matcher from
    // `entity_label_matcher[<key>]=<value>` query parameters.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/routers/matcher/validate",
        ))
        .and(query_param(
            "entity_label_matcher[routing.environment]",
            "prod",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "result": { "success": {} } })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let file_path = std::env::temp_dir().join("cx_test_routers_matcher.json");
    std::fs::write(
        &file_path,
        r#"{"entityLabelMatcher": {"routing.environment": "prod"}}"#,
    )
    .unwrap();

    let target = common::test_target("test-profile", &server.uri());
    let result =
        run_validate_matcher(&[target], file_path.to_str().unwrap(), OutputFormat::Json).await;

    let _ = std::fs::remove_file(&file_path);
    result.expect("run_validate_matcher should send the labels as query params");
}
