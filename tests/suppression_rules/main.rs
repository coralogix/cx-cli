//! Integration tests for `cx alerts suppression-rules`, mainly version-id auto-correction.

#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::suppression_rules::{run_delete, run_get, run_list, run_update};
use coralogix_cli::config::OutputFormat;

const BASE: &str = "/mgmt/openapi/5/alerts/suppression-rules/v1";
const UNIQUE_ID: &str = "38c4a964-a237-41ea-9b02-87af3d734571";
const VERSION_ID: &str = "04b68179-b051-4c2c-a684-ef3a4fb0f80f";
const UNKNOWN_ID: &str = "ffffffff-ffff-ffff-ffff-ffffffffffff";

fn rule_body() -> serde_json::Value {
    json!({
        "alertSchedulerRule": {
            "uniqueIdentifier": UNIQUE_ID,
            "id": VERSION_ID,
            "name": "Maintenance Window",
            "enabled": true,
            "createdAt": "2026-08-10T18:17:04.000Z"
        }
    })
}

fn list_body() -> serde_json::Value {
    json!({
        "alertSchedulerRules": [
            {
                "alertSchedulerRule": {
                    "uniqueIdentifier": UNIQUE_ID,
                    "id": VERSION_ID,
                    "name": "Maintenance Window",
                    "enabled": true
                },
                "nextActiveTimeframes": []
            }
        ]
    })
}

fn empty_list_body() -> serde_json::Value {
    json!({ "alertSchedulerRules": [] })
}

fn write_body_to_temp(name: &str, body: &serde_json::Value) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("cx-supp-test-{name}.json"));
    std::fs::write(&path, serde_json::to_vec(body).unwrap()).expect("write temp body");
    path
}

async fn mock_get(server: &MockServer, id: &str, body: serde_json::Value, times: u64) {
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(times)
        .mount(server)
        .await;
}

fn not_found() -> ResponseTemplate {
    ResponseTemplate::new(404).set_body_json(json!({ "message": "Rule not found" }))
}

async fn mock_get_not_found(server: &MockServer, id: &str, times: u64) {
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/{id}")))
        .respond_with(not_found())
        .expect(times)
        .mount(server)
        .await;
}

async fn mock_delete(server: &MockServer, id: &str, response: ResponseTemplate, times: u64) {
    Mock::given(method("DELETE"))
        .and(path(format!("{BASE}/{id}")))
        .respond_with(response)
        .expect(times)
        .mount(server)
        .await;
}

async fn mock_put(server: &MockServer, response: ResponseTemplate, times: u64) {
    Mock::given(method("PUT"))
        .and(path(BASE))
        .respond_with(response)
        .expect(times)
        .mount(server)
        .await;
}

async fn mock_list(server: &MockServer, body: serde_json::Value, times: u64) {
    Mock::given(method("GET"))
        .and(path(BASE))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(times)
        .mount(server)
        .await;
}

#[tokio::test]
async fn list_unwraps_the_nested_rule_envelope() {
    let server = MockServer::start().await;
    mock_list(&server, list_body(), 1).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list(&targets, OutputFormat::Json)
        .await
        .expect("list should succeed");
}

#[tokio::test]
async fn list_tolerates_an_empty_collection() {
    let server = MockServer::start().await;
    mock_list(&server, json!({}), 1).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list(&targets, OutputFormat::Json)
        .await
        .expect("an absent collection key should not be an error");
}

#[tokio::test]
async fn get_by_unique_identifier_succeeds() {
    let server = MockServer::start().await;
    mock_get(&server, UNIQUE_ID, rule_body(), 1).await;
    mock_list(&server, list_body(), 0).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_get(&targets, UNIQUE_ID, OutputFormat::Json)
        .await
        .expect("get should succeed");
}

#[tokio::test]
async fn get_by_version_id_autocorrects() {
    let server = MockServer::start().await;
    mock_get_not_found(&server, VERSION_ID, 1).await;
    mock_list(&server, list_body(), 1).await;
    mock_get(&server, UNIQUE_ID, rule_body(), 1).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_get(&targets, VERSION_ID, OutputFormat::Json)
        .await
        .expect("get should auto-correct a version id and succeed");
}

#[tokio::test]
async fn get_unknown_id_stays_a_miss() {
    let server = MockServer::start().await;
    mock_get_not_found(&server, UNKNOWN_ID, 1).await;
    mock_list(&server, empty_list_body(), 1).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_get(&targets, UNKNOWN_ID, OutputFormat::Json)
        .await
        .expect("a miss is still a successful call");
}

#[tokio::test]
async fn get_surfaces_non_404_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/{UNIQUE_ID}")))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({ "message": "boom" })))
        .expect(1)
        .mount(&server)
        .await;
    mock_list(&server, list_body(), 0).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let err = run_get(&targets, UNIQUE_ID, OutputFormat::Json)
        .await
        .expect_err("a 500 must not be swallowed as a miss");
    assert!(
        format!("{err:#}").contains("500"),
        "error should carry the status: {err:#}"
    );
}

#[tokio::test]
async fn delete_by_unique_identifier_issues_the_delete() {
    let server = MockServer::start().await;
    mock_get(&server, UNIQUE_ID, rule_body(), 0).await;
    mock_list(&server, list_body(), 0).await;
    mock_delete(
        &server,
        UNIQUE_ID,
        ResponseTemplate::new(200).set_body_json(json!({})),
        1,
    )
    .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_delete(&targets, UNIQUE_ID)
        .await
        .expect("delete should succeed");
}

#[tokio::test]
async fn delete_by_version_id_autocorrects() {
    let server = MockServer::start().await;
    mock_delete(&server, VERSION_ID, not_found(), 1).await;
    mock_list(&server, list_body(), 1).await;
    mock_delete(
        &server,
        UNIQUE_ID,
        ResponseTemplate::new(200).set_body_json(json!({})),
        1,
    )
    .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_delete(&targets, VERSION_ID)
        .await
        .expect("delete should auto-correct a version id and succeed");
}

#[tokio::test]
async fn delete_unknown_id_errors_with_guidance() {
    let server = MockServer::start().await;
    mock_delete(&server, UNKNOWN_ID, not_found(), 1).await;
    mock_list(&server, empty_list_body(), 1).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let err = run_delete(&targets, UNKNOWN_ID)
        .await
        .expect_err("deleting an unresolvable id must not report success");

    let msg = format!("{err:#}");
    assert!(
        msg.contains("No suppression rule found"),
        "error should name the miss: {msg}"
    );
    assert!(
        msg.contains("uniqueIdentifier"),
        "error should point at the right id field: {msg}"
    );
}

#[tokio::test]
async fn update_by_unique_identifier_succeeds() {
    let server = MockServer::start().await;
    mock_list(&server, list_body(), 0).await;
    mock_put(
        &server,
        ResponseTemplate::new(200).set_body_json(rule_body()),
        1,
    )
    .await;

    let body = json!({ "alertSchedulerRule": { "uniqueIdentifier": UNIQUE_ID, "name": "x" } });
    let file = write_body_to_temp("update-ok", &body);

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_update(&targets, file.to_str().unwrap(), OutputFormat::Json)
        .await
        .expect("update by uniqueIdentifier should succeed");
}

#[tokio::test]
async fn update_by_version_id_names_the_id_to_use() {
    let server = MockServer::start().await;
    mock_put(
        &server,
        ResponseTemplate::new(400).set_body_json(json!({ "message": "Invalid UUID format" })),
        1,
    )
    .await;
    mock_list(&server, list_body(), 1).await;

    let body = json!({ "alertSchedulerRule": { "uniqueIdentifier": VERSION_ID, "name": "x" } });
    let file = write_body_to_temp("update-version-id", &body);

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let err = run_update(&targets, file.to_str().unwrap(), OutputFormat::Json)
        .await
        .expect_err("an update keyed by a version id must fail");

    let msg = format!("{err:#}");
    assert!(
        msg.contains("version id"),
        "error should call out the version id: {msg}"
    );
    assert!(
        msg.contains(UNIQUE_ID),
        "error should name the addressable id to use: {msg}"
    );
}

#[tokio::test]
async fn update_rejected_for_another_reason_keeps_the_api_error() {
    let server = MockServer::start().await;
    mock_put(
        &server,
        ResponseTemplate::new(400).set_body_json(json!({ "message": "schedule is required" })),
        1,
    )
    .await;
    mock_list(&server, list_body(), 1).await;

    let body = json!({ "alertSchedulerRule": { "uniqueIdentifier": UNIQUE_ID, "name": "x" } });
    let file = write_body_to_temp("update-bad-body", &body);

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let err = run_update(&targets, file.to_str().unwrap(), OutputFormat::Json)
        .await
        .expect_err("a rejected update must fail");

    let msg = format!("{err:#}");
    assert!(
        msg.contains("schedule is required"),
        "error should be the backend's own: {msg}"
    );
    assert!(
        !msg.contains("version id"),
        "error must not blame the id: {msg}"
    );
}
