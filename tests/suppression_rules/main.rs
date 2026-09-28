//! Integration tests for `cx alerts suppression-rules` (FORGE-710).
//!
//! The group's defining hazard is that a rule has two IDs - `uniqueIdentifier`
//! (stable, addressable) and `id` (the rule version id) - that share a format.
//! GET/DELETE answer 404 for a version id exactly as for an unknown id (since
//! CX-57145; before it they answered a silent 200). These tests pin the
//! version-id auto-correction built on that 404: `get`/`delete` fall back to a
//! `list` lookup and, when the input turns out to be a version id, operate on
//! the real `uniqueIdentifier` instead; a rejected `update` gets the same
//! lookup so its error names the id to use.
//!
//! Console-link coverage for the group lives in `tests/console_urls/main.rs`.

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

/// The list envelope: each rule wrapped one level deeper than the collection
/// key, carrying both IDs. This is what a version-id lookup scans.
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

/// The backend's answer to an id no rule carries - a version id included.
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

/// The list envelope nests each rule one level deeper than the collection key
/// suggests. Modelling it as a flat array deserialized every field to `None`
/// while still exiting 0, so `list` printed a table of blank rows.
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
    // A direct hit resolves on the first GET, so no list lookup should fire.
    mock_get(&server, UNIQUE_ID, rule_body(), 1).await;
    mock_list(&server, list_body(), 0).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_get(&targets, UNIQUE_ID, OutputFormat::Json)
        .await
        .expect("get should succeed");
}

/// Passing the version id gets a 404 first, then the fallback `list` lookup
/// identifies it and `get` re-fetches by the real id.
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

/// A genuinely unknown id 404s on the GET and finds nothing in the list, so it
/// stays a miss (the "Rule not found." path) rather than surfacing a raw 404.
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

/// Before CX-57145 a miss came back as 200 `{}`. It must still read as a miss,
/// not render as a rule.
#[tokio::test]
async fn get_treats_an_empty_body_as_a_miss() {
    let server = MockServer::start().await;
    mock_get(&server, UNKNOWN_ID, json!({}), 1).await;
    mock_list(&server, empty_list_body(), 1).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_get(&targets, UNKNOWN_ID, OutputFormat::Json)
        .await
        .expect("a miss is still a successful call");
}

/// Only a 404 means "no such rule". Any other failure must surface as-is
/// rather than kick off the version-id lookup.
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

/// A hit needs nothing but the DELETE itself - no pre-flight GET, no list.
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

/// Deleting by the version id auto-corrects: the DELETE 404s, the list lookup
/// maps the version id to the real one, and a second DELETE goes to *that* id.
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

/// A delete keyed by an id no rule carries must error with a pointer at the
/// right id field rather than surface a bare 404.
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

/// A good update is just the PUT - the list lookup only runs on a rejection.
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

/// An update body that names the rule by its version id is rejected by the
/// backend; the list lookup then turns that into a message naming the
/// addressable id to use.
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

/// A rejection that isn't about the id (the body names a real rule) must come
/// back as the backend's own error, not a misleading id diagnosis.
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
