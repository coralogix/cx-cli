#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::cases::{
    run_acknowledge, run_assign, run_clear_priority, run_close, run_comment, run_event_get,
    run_events_list, run_get, run_notifications, run_resolve, run_set_priority, run_unassign,
    run_update,
};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn get_case_by_id() {
    let server = MockServer::start().await;

    let body = json!({
        "case": {
            "id": "3f166e9f-3c88-4af2-b52e-138f339dab3e",
            "title": "Database outage investigation",
            "status": "CASE_STATUS_ACKNOWLEDGED",
            "priority": "CASE_PRIORITY_P2",
            "category": "CASE_CATEGORY_AVAILABILITY",
            "createTime": "2025-09-22T10:30:00Z"
        }
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/cases/cases/v1/3f166e9f-3c88-4af2-b52e-138f339dab3e",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_get(
        &targets,
        "3f166e9f-3c88-4af2-b52e-138f339dab3e",
        OutputFormat::Json,
    )
    .await
    .expect("run_get should succeed");
}

#[tokio::test]
async fn assign_case_posts_user_id() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/cases/assigned/v1/case-1"))
        .and(body_partial_json(json!({
            "assignee": { "userId": "user-1" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"case": { "id": "case-1" }})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_assign(&targets, "case-1", "user-1", OutputFormat::Json)
        .await
        .expect("run_assign should succeed");
}

#[tokio::test]
async fn unassign_case_calls_delete() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/mgmt/openapi/5/cases/assigned/v1/case-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"case": { "id": "case-1" }})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_unassign(&targets, "case-1", OutputFormat::Json)
        .await
        .expect("run_unassign should succeed");
}

#[tokio::test]
async fn acknowledge_case_calls_put() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/cases/acknowledged/v1/case-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"case": { "id": "case-1" }})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_acknowledge(&targets, "case-1", OutputFormat::Json)
        .await
        .expect("run_acknowledge should succeed");
}

#[tokio::test]
async fn resolve_case_includes_reason() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/cases/resolved/v1/case-1"))
        .and(body_partial_json(json!({"reason": "False alarm"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"case": { "id": "case-1" }})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_resolve(&targets, "case-1", Some("False alarm"), OutputFormat::Json)
        .await
        .expect("run_resolve should succeed");
}

#[tokio::test]
async fn close_case_posts() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/cases/closed/v1/case-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"case": { "id": "case-1" }})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_close(&targets, "case-1", OutputFormat::Json)
        .await
        .expect("run_close should succeed");
}

#[tokio::test]
async fn set_priority_normalizes_shorthand() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/cases/priority-override/v1/case-1"))
        .and(body_partial_json(json!({"priority": "CASE_PRIORITY_P1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"case": { "id": "case-1" }})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_set_priority(&targets, "case-1", "P1", OutputFormat::Json)
        .await
        .expect("run_set_priority should succeed");
}

#[tokio::test]
async fn clear_priority_calls_delete() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/mgmt/openapi/5/cases/priority-override/v1/case-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"case": { "id": "case-1" }})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_clear_priority(&targets, "case-1", OutputFormat::Json)
        .await
        .expect("run_clear_priority should succeed");
}

#[tokio::test]
async fn update_case_sends_fields_directly() {
    let server = MockServer::start().await;

    // The endpoint takes the updatable fields as the body directly — wrapping
    // them in a `patch` envelope makes the server reject the request as proto
    // with an unknown `patch` field.
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/cases/cases/v1/case-1"))
        .and(body_partial_json(json!({
            "title": "New title"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"case": { "id": "case-1" }})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_update(
        &targets,
        "case-1",
        Some("New title"),
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_update should succeed");
}

#[tokio::test]
async fn update_case_requires_at_least_one_field() {
    let target = common::test_target("test-profile", "http://localhost:0");
    let targets = vec![target];

    let result = run_update(&targets, "case-1", None, None, OutputFormat::Json).await;
    assert!(result.is_err(), "expected update without fields to error");
}

#[tokio::test]
async fn comment_posts_text_to_nested_path() {
    let server = MockServer::start().await;

    // POST /cases/cases/v1/{id}/comments with the comment as the `text` field.
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/cases/cases/v1/case-1/comments"))
        .and(body_partial_json(json!({ "text": "Investigating" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "event": {
                "eventId": "evt-1",
                "eventData": { "comment": { "unsafeText": "Investigating", "attachments": [] } }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_comment(&targets, "case-1", "Investigating", OutputFormat::Json)
        .await
        .expect("run_comment should succeed");
}

#[tokio::test]
async fn events_list_calls_nested_path() {
    let server = MockServer::start().await;

    let body = json!({
        "events": [
            {
                "id": "evt-1",
                "type": "EVENT_TYPE_STATUS_CHANGE",
                "createTime": "2025-09-22T11:00:00Z"
            },
            {
                "id": "evt-2",
                "type": "EVENT_TYPE_COMMENT"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/cases/cases/v1/case-1/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_events_list(&targets, "case-1", OutputFormat::Json)
        .await
        .expect("run_events_list should succeed");
}

#[tokio::test]
async fn event_get_calls_events_path() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/cases/events/v1/evt-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "event": { "id": "evt-1", "type": "EVENT_TYPE_COMMENT" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_event_get(&targets, "evt-1", OutputFormat::Json)
        .await
        .expect("run_event_get should succeed");
}

#[tokio::test]
async fn notifications_lists_deliveries_for_cases() {
    let server = MockServer::start().await;

    // UUID inputs are passed straight through with no resolution round-trip.
    let uuid_1 = "11111111-1111-4111-8111-111111111111";
    let uuid_2 = "22222222-2222-4222-8222-222222222222";

    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/cases/notifications/v1/deliveries"))
        .and(body_partial_json(json!({
            "caseIds": [uuid_1, uuid_2]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "deliveriesByCase": {
                uuid_1: {
                    "notificationDeliveries": [{
                        "timestamp": "2026-06-08T20:15:07Z",
                        "attempted": {
                            "router": { "routerName": "yansa" },
                            "attempts": [{
                                "connector": {
                                    "connectorType": "CONNECTOR_TYPE_SLACK",
                                    "connectorName": "Olly Slack"
                                },
                                "outcome": { "success": { "evidenceUrl": "https://slack/x" } }
                            }]
                        }
                    }]
                },
                uuid_2: {
                    "notificationDeliveries": [{
                        "timestamp": "2026-06-08T20:13:50Z",
                        "noRouterMatched": {}
                    }]
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let ids = vec![uuid_1.to_string(), uuid_2.to_string()];
    run_notifications(&targets, &ids, OutputFormat::Text)
        .await
        .expect("run_notifications should succeed");
}

#[tokio::test]
async fn notifications_accepts_readable_case_id_directly() {
    let server = MockServer::start().await;

    // The deliveries endpoint accepts readable IDs directly — no GET on the
    // case to resolve a UUID first.
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/cases/notifications/v1/deliveries"))
        .and(body_partial_json(json!({
            "caseIds": ["CASE-764019"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "deliveriesByCase": {
                "CASE-764019": {
                    "notificationDeliveries": [
                        { "connectorType": "CONNECTOR_TYPE_SLACK", "status": "DELIVERED" }
                    ]
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let ids = vec!["CASE-764019".to_string()];
    run_notifications(&targets, &ids, OutputFormat::Json)
        .await
        .expect("run_notifications should accept readable IDs directly");
}

#[tokio::test]
async fn assign_case_resolves_email_to_user_id() {
    let server = MockServer::start().await;

    // Teammates directory lookup must come first so the email can be resolved.
    Mock::given(method("GET"))
        .and(path("/api/v1/user/team/teammates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                { "id": "uid-alice", "username": "alice@example.com" },
                { "id": "uid-bob", "username": "bob@example.com" }
            ]
        })))
        .mount(&server)
        .await;

    // Then the assign call uses the resolved user ID, not the email.
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/cases/assigned/v1/case-1"))
        .and(body_partial_json(json!({
            "assignee": { "userId": "uid-alice" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "case": { "id": "case-1", "assignee": { "userId": "uid-alice" } }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_assign(&targets, "case-1", "alice@example.com", OutputFormat::Json)
        .await
        .expect("run_assign with email should succeed");
}
