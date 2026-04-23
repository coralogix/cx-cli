mod common;

use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::alerts::{run_disable, run_enable, run_get, run_list};
use cx::config::OutputFormat;

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
        .and(path("/mgmt/openapi/latest/alerts/alerts-general/v3"))
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
        .and(path("/mgmt/openapi/latest/alerts/alerts-general/v3"))
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
        .and(path(
            "/mgmt/openapi/latest/alerts/alerts-general/v3/alert-001",
        ))
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
        .and(path(
            "/mgmt/openapi/latest/alerts/alerts-general/v3/ver-123",
        ))
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
        .and(path(
            "/mgmt/openapi/latest/alerts/alerts-general/v3/alert-version-id/ver-123",
        ))
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

    Mock::given(method("POST"))
        .and(path(
            "/mgmt/openapi/latest/alerts/alerts-general/v3/alert-001:setActive",
        ))
        .and(query_param("active", "true"))
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
async fn disable_alert() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(
            "/mgmt/openapi/latest/alerts/alerts-general/v3/alert-001:setActive",
        ))
        .and(query_param("active", "false"))
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
