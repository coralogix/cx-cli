#[path = "../common/mod.rs"]
mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use assert_cmd::Command as AssertCommand;
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

// ── Regression: --name-filter and the alerts-page console link ──────────────
//
// These spawn the real `cx` binary (rather than calling `run_list` directly)
// because the "View in Coralogix" link is printed via a plain `eprintln!` in
// `render::print_console_link` - the only way to observe it is to capture a
// subprocess's real stderr, same as `tests/profile_override/main.rs`.

static ALERTS_LINK_TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_home() -> PathBuf {
    let id = ALERTS_LINK_TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cx_alerts_link_test_{pid}_{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_profile(home: &std::path::Path, name: &str, region: &str) {
    let profiles_dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    let content = format!(
        r#"auth = "api_key"
credential_storage = "file"
api_key = "test-api-key"
region = "{region}"
"#
    );
    fs::write(profiles_dir.join(format!("{name}.toml")), content).unwrap();
}

fn write_config(home: &std::path::Path, default_profile: &str) {
    let cx_dir = home.join(".cx");
    fs::create_dir_all(&cx_dir).unwrap();
    fs::write(
        cx_dir.join("config.toml"),
        format!("default_profile = \"{default_profile}\"\n"),
    )
    .unwrap();
}

fn cx(home: &std::path::Path) -> AssertCommand {
    let mut cmd = AssertCommand::cargo_bin("cx").expect("cx binary should build");
    cmd.env("CX_HOME", home);
    cmd.env_remove("CX_API_KEY");
    cmd.env_remove("CX_REGION");
    cmd.env_remove("CX_PROFILE");
    cmd
}

async fn mount_whoami_and_alerts(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/identity/whoami"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "team_url": "https://test-team.example.com"
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertDefs": [
                { "id": "alert-001", "name": "High Error Rate" },
                { "id": "alert-002", "name": "CPU Spike" }
            ]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn name_filter_matching_nothing_suppresses_alerts_page_link() {
    let server = MockServer::start().await;
    mount_whoami_and_alerts(&server).await;

    let home = temp_home();
    write_profile(&home, "default", &server.uri());
    write_config(&home, "default");

    let output = cx(&home)
        .args(["alerts", "list", "--name-filter", "no-such-alert"])
        .output()
        .expect("cx should run");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("View in Coralogix"),
        "a filter matching zero alerts should not print the alerts-page link, stderr: {stderr}"
    );
}

#[tokio::test]
async fn name_filter_matching_something_still_prints_alerts_page_link() {
    let server = MockServer::start().await;
    mount_whoami_and_alerts(&server).await;

    let home = temp_home();
    write_profile(&home, "default", &server.uri());
    write_config(&home, "default");

    let output = cx(&home)
        .args(["alerts", "list", "--name-filter", "cpu"])
        .output()
        .expect("cx should run");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix"),
        "a filter matching at least one alert should still print the alerts-page link, stderr: {stderr}"
    );
}
