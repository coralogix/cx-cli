#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::retentions::{run_list, run_status};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn retentions_list_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "retentions": [
            {"id": "ret-001", "name": "Hot Storage", "retentionDays": 30, "enabled": true}
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataengine/retention-tags/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_list(&targets, OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}

#[tokio::test]
async fn retentions_status_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataengine/retention-tags/v1/enabled"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"enabled": true})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_status(&targets, OutputFormat::Json)
        .await
        .expect("run_status should succeed");
}

#[tokio::test]
async fn retentions_update_posts_to_retention_tags() {
    use wiremock::matchers::body_json;

    let server = MockServer::start().await;
    let body = json!({
        "retentionUpdateElements": [{ "id": "ret-002", "name": "Short" }]
    });
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dataengine/retention-tags/v1"))
        .and(body_json(&body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "retentions": [{ "id": "ret-002", "order": 2, "name": "Short", "editable": true }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file =
        std::env::temp_dir().join(format!("cx-retentions-update-{}.json", std::process::id()));
    std::fs::write(&file, body.to_string()).unwrap();
    let targets = vec![common::test_target("test-profile", &server.uri())];
    let result = coralogix_cli::commands::retentions::run_update(
        &targets,
        file.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await;
    let _ = std::fs::remove_file(&file);
    result.expect("run_update should succeed");
}
