#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::alerts::{
    run_delete, run_disable, run_enable, run_events, run_get, run_list,
};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_alerts_returns_items_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "alertDefs": [
            {
                "id": "alert-001",
                "name": "High Error Rate",
                "enabled": true,
                "priority": "ALERT_DEF_PRIORITY_P2",
                "type": "ALERT_DEF_TYPE_LOGS_THRESHOLD",
                "status": "OK",
                "updatedTime": "2024-06-01T12:00:00Z"
            },
            {
                "id": "alert-002",
                "name": "CPU Spike",
                "enabled": false,
                "priority": "ALERT_DEF_PRIORITY_P1",
                "type": "ALERT_DEF_TYPE_METRIC_THRESHOLD",
                "status": "ALERTING"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_list(&targets, None, OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}

#[tokio::test]
async fn list_alerts_with_name_filter() {
    let server = MockServer::start().await;

    let body = json!({
        "alertDefs": [
            { "id": "alert-001", "name": "High Error Rate" },
            { "id": "alert-002", "name": "CPU Spike" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    // Filter should not error even if it filters everything out
    run_list(&targets, Some("cpu"), OutputFormat::Json)
        .await
        .expect("run_list with filter should succeed");
}

#[tokio::test]
async fn get_alert_by_id() {
    let server = MockServer::start().await;

    let body = json!({
        "alertDef": {
            "id": "alert-001",
            "name": "High Error Rate",
            "enabled": true,
            "priority": "ALERT_DEF_PRIORITY_P2",
            "type": "ALERT_DEF_TYPE_LOGS_THRESHOLD",
            "status": "OK"
        }
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/alert-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_get(&targets, "alert-001", OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}

#[tokio::test]
async fn get_alert_falls_back_to_version_id_on_404() {
    let server = MockServer::start().await;

    // Primary endpoint returns 404
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/ver-123"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "not found"})))
        .expect(1)
        .mount(&server)
        .await;

    // Fallback version-id endpoint succeeds
    let body = json!({
        "alertDef": {
            "id": "alert-real-id",
            "name": "Found by version"
        }
    });
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/version-ids/ver-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_get(&targets, "ver-123", OutputFormat::Json)
        .await
        .expect("run_get should fall back to version ID");
}

#[tokio::test]
async fn enable_alert() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/alert-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertDef": {
                "id": "alert-001",
                "alertDefProperties": {
                    "name": "Test",
                    "enabled": false,
                    "priority": "ALERT_DEF_PRIORITY_P5"
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .and(body_json(json!({
            "id": "alert-001",
            "alertDefProperties": {
                "name": "Test",
                "enabled": true,
                "priority": "ALERT_DEF_PRIORITY_P5_OR_UNSPECIFIED"
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_enable(&targets, "alert-001")
        .await
        .expect("run_enable should succeed");
}

#[tokio::test]
async fn delete_alert() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/alert-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_delete(&targets, "alert-001")
        .await
        .expect("run_delete should succeed");
}

#[tokio::test]
async fn disable_alert() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/alert-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertDef": {
                "id": "alert-001",
                "alertDefProperties": {
                    "name": "Test",
                    "enabled": true
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .and(body_json(json!({
            "id": "alert-001",
            "alertDefProperties": {
                "name": "Test",
                "enabled": false
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_disable(&targets, "alert-001")
        .await
        .expect("run_disable should succeed");
}

#[tokio::test]
async fn events_without_alert_version_ids_uses_general_events_endpoint() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/events/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "events": [
                {
                    "cxEventKey": "event-1",
                    "cxEventType": "test",
                    "cxEventTimestamp": "1714857600"
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_events(&targets, &[], None, None, OutputFormat::Json)
        .await
        .expect("run_events should use general events endpoint");
}

#[tokio::test]
async fn events_without_alert_version_ids_paginates_general_events_endpoint() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/events/v3"))
        .and(query_param_is_missing("pagination.page_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "events": [
                { "cxEventKey": "event-1" }
            ],
            "pagination": {
                "nextPageToken": "page-2"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/events/v3"))
        .and(query_param("pagination.page_token", "page-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "events": [
                { "cxEventKey": "event-2" }
            ],
            "pagination": {}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_events(&targets, &[], None, None, OutputFormat::Json)
        .await
        .expect("run_events should paginate general events endpoint");
}

#[tokio::test]
async fn events_with_alert_version_ids_uses_scoped_endpoint() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/all/events"))
        .and(query_param("alert_ids", "version-001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "events": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];
    let ids = vec!["version-001".to_string()];

    run_events(&targets, &ids, None, None, OutputFormat::Json)
        .await
        .expect("run_events should use scoped alert events endpoint");
}

#[tokio::test]
async fn events_with_alert_version_ids_paginates_scoped_endpoint() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/all/events"))
        .and(query_param("alert_ids", "version-001"))
        .and(query_param_is_missing("pagination.page_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertEvents": [
                { "cxEventKey": "event-1" }
            ],
            "pagination": {
                "nextPageToken": "page-2"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/all/events"))
        .and(query_param("alert_ids", "version-001"))
        .and(query_param("pagination.page_token", "page-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertEvents": [
                { "cxEventKey": "event-2" }
            ],
            "pagination": {}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];
    let ids = vec!["version-001".to_string()];

    run_events(&targets, &ids, None, None, OutputFormat::Json)
        .await
        .expect("run_events should paginate scoped alert events endpoint");
}
