#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::configuration_overlays;
use coralogix_cli::config::OutputFormat;

const BASE: &str = "/mgmt/openapi/5/fleet-management/configuration-overlays/v1";

fn write_json_file(value: &serde_json::Value) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("cx-cli-overlay-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

#[tokio::test]
async fn list_configuration_overlays_forwards_pagination_and_archived_filter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BASE))
        .and(query_param("include_archived", "true"))
        .and(query_param("page_size", "25"))
        .and(query_param("page_token", "opaque cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "overlays": [{"id": "overlay-1", "active": false, "custom": {"targets": []}}],
            "nextPageToken": "next cursor"
        })))
        .expect(1)
        .mount(&server)
        .await;

    configuration_overlays::run_list(
        &[common::test_target("test-profile", &server.uri())],
        true,
        Some(25),
        Some("opaque cursor"),
        OutputFormat::Json,
    )
    .await
    .expect("list should succeed");
}

#[tokio::test]
async fn get_configuration_overlay_can_include_archived_resource() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/overlay-1")))
        .and(query_param("include_archived", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "overlay-1",
            "custom": {"configuration": "service: {}", "targets": []},
            "schedules": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    configuration_overlays::run_get(
        &[common::test_target("test-profile", &server.uri())],
        "overlay-1",
        true,
        OutputFormat::Json,
    )
    .await
    .expect("get should succeed");
}

#[tokio::test]
async fn create_configuration_overlay_posts_complete_body() {
    let server = MockServer::start().await;
    let body = json!({
        "name": "Debug logs",
        "active": false,
        "custom": {"configuration": "service:\n  telemetry:\n    logs:\n      level: debug", "targets": []}
    });
    let file = write_json_file(&body);

    Mock::given(method("POST"))
        .and(path(BASE))
        .and(body_json(body.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "overlay-1"})))
        .expect(1)
        .mount(&server)
        .await;

    configuration_overlays::run_create(
        &[common::test_target("test-profile", &server.uri())],
        file.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await
    .expect("create should succeed");
    std::fs::remove_file(file).unwrap();
}

#[tokio::test]
async fn update_configuration_overlay_uses_patch_and_archive_uses_post() {
    let server = MockServer::start().await;
    let body = json!({"description": "Updated"});
    let file = write_json_file(&body);

    Mock::given(method("PATCH"))
        .and(path(format!("{BASE}/overlay-1")))
        .and(query_param("update_mask", "description"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "overlay-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/overlay-1/archive")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    configuration_overlays::run_update(
        std::slice::from_ref(&target),
        "overlay-1",
        "description",
        file.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await
    .expect("update should succeed");
    std::fs::remove_file(file).unwrap();
    configuration_overlays::run_archive(&[target], "overlay-1")
        .await
        .expect("archive should succeed");
}
