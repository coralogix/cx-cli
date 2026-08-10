//! Integration tests for `cx alerts suppression-rules` (FORGE-710).
//!
//! The group's defining hazard is that a rule has two IDs - `uniqueIdentifier`
//! (stable, addressable) and `id` (the rule version id) - and the API fails
//! *silently* when they're confused: an unknown id gets 200 `{}` from GET and
//! a 200 no-op from DELETE, never a 404. These tests pin the behaviour that
//! turns those silent misses into visible ones.
//!
//! Console-link coverage for the group lives in `tests/console_urls/main.rs`.

#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::suppression_rules::{run_delete, run_get, run_list};
use coralogix_cli::config::OutputFormat;

const BASE: &str = "/mgmt/openapi/5/alerts/suppression-rules/v1";
const UNIQUE_ID: &str = "38c4a964-a237-41ea-9b02-87af3d734571";
const VERSION_ID: &str = "04b68179-b051-4c2c-a684-ef3a4fb0f80f";

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

/// The list envelope nests each rule one level deeper than the collection key
/// suggests. Modelling it as a flat array deserialized every field to `None`
/// while still exiting 0, so `list` printed a table of blank rows.
#[tokio::test]
async fn list_unwraps_the_nested_rule_envelope() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(BASE))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
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
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list(&targets, OutputFormat::Json)
        .await
        .expect("list should succeed");
}

#[tokio::test]
async fn list_tolerates_an_empty_collection() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(BASE))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list(&targets, OutputFormat::Json)
        .await
        .expect("an absent collection key should not be an error");
}

#[tokio::test]
async fn get_by_unique_identifier_succeeds() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/{UNIQUE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(rule_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_get(&targets, UNIQUE_ID, OutputFormat::Json)
        .await
        .expect("get should succeed");
}

/// A miss is a 200 with an empty body, not a 404. `get` treats it as "not
/// found" rather than rendering `{}` as though it were a rule.
#[tokio::test]
async fn get_by_version_id_is_a_miss_not_an_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/{VERSION_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_get(&targets, VERSION_ID, OutputFormat::Json)
        .await
        .expect("a miss is still a successful call");
}

#[tokio::test]
async fn delete_by_unique_identifier_issues_the_delete() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/{UNIQUE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(rule_body()))
        .expect(1)
        .mount(&server)
        .await;
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

/// The dangerous case. DELETE answers 200 for an id it doesn't know without
/// removing anything, so a delete keyed by the version id used to print
/// "Deleted rule ..." while the rule stayed put. The pre-flight GET must turn
/// that into an error - and must stop the DELETE from being sent at all.
#[tokio::test]
async fn delete_by_version_id_errors_instead_of_silently_no_opping() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/{VERSION_ID}")))
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
    let err = run_delete(&targets, VERSION_ID)
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
