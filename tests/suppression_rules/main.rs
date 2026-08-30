//! Integration tests for `cx alerts suppression-rules` (FORGE-710).
//!
//! The group's defining hazard is that a rule has two IDs - `uniqueIdentifier`
//! (stable, addressable) and `id` (the rule version id) - and the API fails
//! *silently* when they're confused: an unknown id gets 200 `{}` from GET and
//! a 200 no-op from DELETE, never a 404. These tests pin the behaviour that
//! turns those silent misses into visible ones, including the version-id
//! auto-correction: `get`/`delete` fall back to a `list` lookup and, when the
//! input turns out to be a version id, operate on the real `uniqueIdentifier`
//! instead; `update` detects the same mistake and errors before the PUT.
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

/// Passing the version id lands on the empty `{}` first, then the fallback
/// `list` lookup identifies it and `get` re-fetches by the real id.
#[tokio::test]
async fn get_by_version_id_autocorrects() {
    let server = MockServer::start().await;
    mock_get(&server, VERSION_ID, json!({}), 1).await;
    mock_list(&server, list_body(), 1).await;
    mock_get(&server, UNIQUE_ID, rule_body(), 1).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_get(&targets, VERSION_ID, OutputFormat::Json)
        .await
        .expect("get should auto-correct a version id and succeed");
}

/// A genuinely unknown id misses on the GET and finds nothing in the list, so
/// it stays a miss (the "Rule not found." path) rather than erroring the call.
#[tokio::test]
async fn get_unknown_id_stays_a_miss() {
    let server = MockServer::start().await;
    mock_get(&server, UNKNOWN_ID, json!({}), 1).await;
    mock_list(&server, empty_list_body(), 1).await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_get(&targets, UNKNOWN_ID, OutputFormat::Json)
        .await
        .expect("a miss is still a successful call");
}

#[tokio::test]
async fn delete_by_unique_identifier_issues_the_delete() {
    let server = MockServer::start().await;
    mock_get(&server, UNIQUE_ID, rule_body(), 1).await;
    mock_list(&server, list_body(), 0).await;
    Mock::given(method("DELETE"))
        .and(path(format!("{BASE}/{UNIQUE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_delete(&targets, UNIQUE_ID)
        .await
        .expect("delete should succeed");
}

/// Deleting by the version id auto-corrects: the pre-flight GET misses, the
/// list lookup maps it to the real id, and the DELETE goes to *that* id. The
/// version-id DELETE path must never be hit.
#[tokio::test]
async fn delete_by_version_id_autocorrects() {
    let server = MockServer::start().await;
    mock_get(&server, VERSION_ID, json!({}), 1).await;
    mock_list(&server, list_body(), 1).await;
    Mock::given(method("DELETE"))
        .and(path(format!("{BASE}/{UNIQUE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("{BASE}/{VERSION_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_delete(&targets, VERSION_ID)
        .await
        .expect("delete should auto-correct a version id and succeed");
}

/// A delete keyed by an id no rule carries must error rather than report a
/// 200 no-op as success, and must not send the DELETE at all.
#[tokio::test]
async fn delete_unknown_id_errors_instead_of_silently_no_opping() {
    let server = MockServer::start().await;
    mock_get(&server, UNKNOWN_ID, json!({}), 1).await;
    mock_list(&server, empty_list_body(), 1).await;
    Mock::given(method("DELETE"))
        .and(path(format!("{BASE}/{UNKNOWN_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .mount(&server)
        .await;

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
    mock_list(&server, list_body(), 1).await;
    Mock::given(method("PUT"))
        .and(path(BASE))
        .respond_with(ResponseTemplate::new(200).set_body_json(rule_body()))
        .expect(1)
        .mount(&server)
        .await;

    let body = json!({ "alertSchedulerRule": { "uniqueIdentifier": UNIQUE_ID, "name": "x" } });
    let file = write_body_to_temp("update-ok", &body);

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_update(&targets, file.to_str().unwrap(), OutputFormat::Json)
        .await
        .expect("update by uniqueIdentifier should succeed");
}

/// An update body that names the rule by its version id is caught by the list
/// lookup and errored before the PUT, turning the backend's field-less
/// "400 Invalid UUID format" into an actionable message.
#[tokio::test]
async fn update_by_version_id_errors_before_sending() {
    let server = MockServer::start().await;
    mock_list(&server, list_body(), 1).await;
    Mock::given(method("PUT"))
        .and(path(BASE))
        .respond_with(ResponseTemplate::new(200).set_body_json(rule_body()))
        .expect(0)
        .mount(&server)
        .await;

    let body = json!({ "alertSchedulerRule": { "uniqueIdentifier": VERSION_ID, "name": "x" } });
    let file = write_body_to_temp("update-version-id", &body);

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let err = run_update(&targets, file.to_str().unwrap(), OutputFormat::Json)
        .await
        .expect_err("an update keyed by a version id must not be sent");

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
