//! Integration tests for "View in Coralogix" console links (FORGE-586).
//!
//! After a successful `dashboards create`/`replace`, `alerts create`,
//! `views create`/`update`, or `cases` lifecycle mutation, the CLI should print a
//! `View in Coralogix: <url>` line to stderr - purely informational, never
//! affecting `-o json` / `-o agents` stdout.
//!
//! These tests spawn the real `cx` binary (via `assert_cmd`) against a
//! `wiremock` server so stdout and stderr can be asserted independently,
//! which isn't possible when calling library functions in-process (their
//! `eprintln!`/`println!` go to the test harness's own stdio, not a
//! capturable buffer).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_home() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("cx_console_url_test_{}_{id}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn temp_json_path(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "cx_console_url_test_{label}_{}.json",
        std::process::id()
    ));
    p
}

/// Write a profile pointing at `base_url`, optionally with an explicit
/// `console_url` override.
fn write_profile(home: &std::path::Path, name: &str, base_url: &str, console_url: Option<&str>) {
    let profiles_dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    let mut content = format!(
        r#"auth = "api_key"
credential_storage = "file"
api_key = "test-key"
region = "{base_url}"
"#
    );
    if let Some(url) = console_url {
        content.push_str(&format!("console_url = \"{url}\"\n"));
    }
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

fn cx(home: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("CX_HOME", home);
    cmd.env_remove("CX_API_KEY");
    cmd.env_remove("CX_REGION");
    cmd.env_remove("CX_PROFILE");
    cmd
}

// ── dashboards create ────────────────────────────────────────────────────────

#[tokio::test]
async fn dashboard_create_prints_console_link_when_console_url_configured() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"dashboardId": "dash-abc123"})),
        )
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("dash_create");
    fs::write(
        &file_path,
        r#"{"name": "Demo Dashboard", "layout": {"sections": []}}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "dashboards",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    // whoami must not have been called - the explicit console_url overrides it.
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| r.url.path() != "/identity/whoami"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn dashboard_replace_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"dashboardId": "dash-abc123"})),
        )
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("dash_replace");
    fs::write(
        &file_path,
        r#"{"id": "dash-abc123", "name": "Demo Dashboard", "layout": {"sections": []}}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "dashboards",
            "replace",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn dashboard_replace_falls_back_to_request_id_when_response_has_no_id() {
    // Some deployments return an empty body on a successful replace. The CLI
    // already knows the dashboard's id from the request itself, so it should
    // still be able to print the console link rather than silently skipping it.
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("dash_replace_empty_resp");
    fs::write(
        &file_path,
        r#"{"id": "dash-abc123", "name": "Demo Dashboard", "layout": {"sections": []}}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "dashboards",
            "replace",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123"
        ),
        "stderr did not contain the console link (fallback to request id failed): {stderr}"
    );
    assert!(
        !stderr.contains("did not include an ID"),
        "should not warn about a missing ID once it fell back to the request's own id: {stderr}"
    );

    // The API echoed back an empty `{}`, so -o json has nothing to attach the
    // link to - it should stay empty rather than becoming a redundant
    // `{"consoleUrl": ...}` that just repeats the stderr line above.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("consoleUrl"),
        "stdout should not carry a consoleUrl-only payload when the response was empty: {stdout}"
    );
}

// ── alerts create ────────────────────────────────────────────────────────────

#[tokio::test]
async fn alert_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertDef": {
                "id": "alert-xyz789",
                "alertDefProperties": {
                    "name": "Demo Alert",
                    "priority": "ALERT_DEF_PRIORITY_P2",
                    "enabled": true,
                    "type": "ALERT_DEF_TYPE_LOGS_THRESHOLD"
                }
            }
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("alert_create");
    fs::write(
        &file_path,
        r#"{"alertDefProperties": {"name": "Demo Alert", "priority": "ALERT_DEF_PRIORITY_P2", "enabled": true}}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "alerts",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/alerts/alert-xyz789"),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── views create/update ──────────────────────────────────────────────────────

#[tokio::test]
async fn view_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/data-exploration/views/v1/views"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "view": {"id": "view-123", "name": "Demo View"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("view_create");
    fs::write(&file_path, r#"{"name": "Demo View"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "views",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/explore?viewId=view-123"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn view_create_prints_console_link_with_bare_response() {
    // Some deployments return the created view directly (no `{"view": ...}` envelope).
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/data-exploration/views/v1/views"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "view-456", "name": "Demo View"})),
        )
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("view_create_bare");
    fs::write(&file_path, r#"{"name": "Demo View"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "views",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/explore?viewId=view-456"
        ),
        "stderr did not contain the console link: {stderr}"
    );

    let stdout: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON ({e}): {:?}", output.stdout));
    // A single result unwraps to a bare object rather than a one-element array.
    assert_eq!(
        stdout["id"], "view-456",
        "created view should be present, not dropped: {stdout}"
    );
    assert_eq!(
        stdout["consoleUrl"],
        "https://c4c.app.eu2.coralogix.com/explore?viewId=view-456"
    );
}

#[tokio::test]
async fn view_get_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/data-exploration/views/v1/views/view-123",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "view": {"id": "view-123", "name": "Demo View"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "views", "get", "view-123"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/explore?viewId=view-123"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn view_update_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path(
            "/mgmt/openapi/5/data-exploration/views/v1/views/view-123",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "view-123", "name": "Demo View"})),
        )
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("view_update");
    fs::write(&file_path, r#"{"name": "Demo View"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "views",
            "update",
            "view-123",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/explore?viewId=view-123"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── cases lifecycle ──────────────────────────────────────────────────────────

#[tokio::test]
async fn case_resolve_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/cases/resolved/v1/case-777"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "case": {
                "id": "case-777",
                "readableId": "CASE-42",
                "title": "Checkout errors",
                "status": "CASE_STATUS_RESOLVED",
                "priority": "CASE_PRIORITY_P2"
            }
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "cases",
            "resolve",
            "case-777",
            "--reason",
            "fixed",
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/cases?id=case-777"),
        "stderr did not contain the console link: {stderr}"
    );
}

/// `cases notifications` keeps its "Evidence URL" column in the text table
/// (the presigned per-attempt URL is the only pointer to that specific
/// delivery's evidence - a generic link to the Cases page isn't a substitute)
/// and does not print a `View in Coralogix` link, since no per-list console
/// page for notification deliveries is confirmed.
#[tokio::test]
async fn case_notifications_keeps_evidence_url_column_and_prints_no_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/cases/notifications/v1/deliveries"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "deliveriesByCase": {
                "case-777": {
                    "notificationDeliveries": [{
                        "timestamp": "2026-06-08T20:15:07Z",
                        "attempted": {
                            "router": { "routerName": "yansa" },
                            "attempts": [{
                                "connector": {
                                    "connectorType": "CONNECTOR_TYPE_SLACK",
                                    "connectorName": "Olly Slack"
                                },
                                "outcome": {
                                    "success": {
                                        "evidenceUrl": "https://slack.example.com/evidence/xyz"
                                    }
                                }
                            }]
                        }
                    }]
                },
                "case-888": {
                    "notificationDeliveries": [{
                        "timestamp": "2026-06-08T20:13:50Z",
                        "noRouterMatched": {}
                    }]
                }
            }
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    // Text mode: the "Evidence URL" column and its value are present, and no
    // console link is printed to stderr.
    let text = cx(&home)
        .args([
            "--profile",
            "mock",
            "cases",
            "notifications",
            "case-777",
            "case-888",
            "-o",
            "text",
        ])
        .output()
        .expect("failed to run cx");
    assert!(text.status.success(), "{:?}", text);

    let text_stdout = String::from_utf8_lossy(&text.stdout);
    assert!(
        text_stdout.contains("Evidence URL"),
        "text table should have an Evidence URL column: {text_stdout}"
    );
    assert!(
        text_stdout.contains("https://slack.example.com/evidence/xyz"),
        "text table should print the evidence URL: {text_stdout}"
    );
    let text_stderr = String::from_utf8_lossy(&text.stderr);
    assert!(
        !text_stderr.contains("View in Coralogix:"),
        "no console link should be printed for cases notifications: {text_stderr}"
    );

    // JSON mode: the evidence URL is still carried in the payload.
    let json = cx(&home)
        .args([
            "--profile",
            "mock",
            "cases",
            "notifications",
            "case-777",
            "case-888",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to run cx");
    assert!(json.status.success(), "{:?}", json);
    let json_stdout = String::from_utf8_lossy(&json.stdout);
    assert!(
        json_stdout.contains("https://slack.example.com/evidence/xyz"),
        "-o json must keep the evidence URL: {json_stdout}"
    );
}

// ── No console_url override -> resolved via /identity/whoami ────────────────

/// With no explicit `console_url`, the console link is resolved by calling
/// `/identity/whoami` and using its `team_url` verbatim - no region/domain
/// table or team-name guessing involved.
#[tokio::test]
async fn dashboard_create_prints_console_link_from_whoami_team_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/identity/whoami"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "team_id": 1,
            "team_url": "https://c4c.app.eu2.coralogix.com"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"dashboardId": "dash-abc123"})),
        )
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(&home, "mock", &server.uri(), None);
    write_config(&home, "mock");

    let file_path = temp_json_path("dash_whoami_link");
    fs::write(
        &file_path,
        r#"{"name": "Demo Dashboard", "layout": {"sections": []}}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "dashboards",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

/// When `/identity/whoami` returns no usable `team_url` (and no
/// `console_url` override is set), `cx` fails quietly: no link, no
/// explanatory hint, command still succeeds.
#[tokio::test]
async fn no_console_link_when_whoami_has_no_team_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/identity/whoami"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"team_id": 1})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"dashboardId": "dash-abc123"})),
        )
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(&home, "mock", &server.uri(), None);
    write_config(&home, "mock");

    let file_path = temp_json_path("dash_no_link");
    fs::write(
        &file_path,
        r#"{"name": "Demo Dashboard", "layout": {"sections": []}}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "dashboards",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("View in Coralogix"),
        "unexpected console link/hint with no usable team_url: {stderr}"
    );
    assert!(
        stderr.contains("Created dashboard 'Demo Dashboard' (ID: dash-abc123)"),
        "expected the usual success line, stderr: {stderr}"
    );
}

// ── stdout carries the console link as a `consoleUrl` field ──────────────────

/// `-o json` output must embed the same URL that's echoed to stderr as a
/// `consoleUrl` field on the result object - see `render::tag_console_url`.
/// This intentionally supersedes an earlier version of this test
/// (`json_output_is_unaffected_by_console_link`) which asserted the opposite:
/// that stdout was untouched by the console-link feature. Per reviewer
/// feedback, agent/script consumers of `-o json` / `-o agents` need the link
/// in the structured payload too, not only as a human-readable stderr line.
#[tokio::test]
async fn json_output_includes_console_url_field() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"dashboardId": "dash-abc123"})),
        )
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("dash_json_output");
    fs::write(
        &file_path,
        r#"{"name": "Demo Dashboard", "layout": {"sections": []}}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "dashboards",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    // stdout is the API response plus a `consoleUrl` field - no
    // "View in Coralogix: " prefix text (that's stderr-only phrasing).
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(
        parsed,
        json!({
            "dashboardId": "dash-abc123",
            "consoleUrl": "https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123",
        })
    );
    assert!(!stdout.contains("View in Coralogix"));

    // The human-readable line still goes to stderr too, with the same URL.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr
        .contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123"));
}

/// Regression test for the single- vs. multi-profile path invariant: on a
/// wrapper-shaped `get` (`{"alertDef": {...}}`), the console link must nest
/// inside the wrapper (`.alertDef.consoleUrl`) in *both* modes. In
/// multi-profile mode `_profile` is added at the root first, and an earlier
/// version of `tag_console_url` treated that as a second top-level field and
/// pushed the link to `.consoleUrl` instead - silently moving it for scripts
/// as soon as a second profile was added.
#[tokio::test]
async fn multi_profile_get_nests_console_link_inside_wrapper() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/alert-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertDef": {"id": "alert-1", "name": "My Alert"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "prod",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_profile(
        &home,
        "staging",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "prod");

    let output = cx(&home)
        .args([
            "--profile",
            "prod",
            "--profile",
            "staging",
            "alerts",
            "get",
            "alert-1",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nstdout: {stdout}"));

    let results = parsed.as_array().expect("multi-profile output is an array");
    assert_eq!(results.len(), 2, "expected one result per profile");
    for result in results {
        assert_eq!(
            result["alertDef"]["consoleUrl"], "https://c4c.app.eu2.coralogix.com/alerts/alert-1",
            "link must nest inside the wrapper, same path as single-profile: {result}"
        );
        assert_eq!(
            result["consoleUrl"],
            serde_json::Value::Null,
            "link must not leak to the root when `_profile` is present: {result}"
        );
        assert!(
            result["_profile"].is_string(),
            "profile tag preserved: {result}"
        );
    }
}

// ── e2m create/update ────────────────────────────────────────────────────────

#[tokio::test]
async fn e2m_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/events2metrics/events2metrics/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "e2m": {"id": "e2m-new", "name": "New E2M"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("e2m_create");
    fs::write(&file_path, r#"{"name": "New E2M"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "e2m",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/tco/metrics/e2m-new"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn e2m_get_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/events2metrics/events2metrics/v2/e2m-123",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "e2m": {"id": "e2m-123", "name": "Demo E2M"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "e2m", "get", "e2m-123"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/tco/metrics/e2m-123"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn e2m_update_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/events2metrics/events2metrics/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "e2m": {"id": "e2m-1", "name": "Updated E2M"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("e2m_update");
    fs::write(&file_path, r#"{"id": "e2m-1", "name": "Updated E2M"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "e2m",
            "update",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/tco/metrics/e2m-1"),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── slos create/update ───────────────────────────────────────────────────────

#[tokio::test]
async fn slo_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/slo/slos/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "slo": {"id": "slo-new", "name": "New SLO"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("slo_create");
    fs::write(&file_path, r#"{"name": "New SLO"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "slos",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr
            .contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/slo/slo-new/overview"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn slo_update_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/slo/slos/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "slo": {"id": "slo-1", "name": "Updated SLO"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("slo_update");
    fs::write(&file_path, r#"{"id": "slo-1", "name": "Updated SLO"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "slos",
            "update",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/slo/slo-1/overview"),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── notifications connectors create/update ───────────────────────────────────

#[tokio::test]
async fn connector_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/connectors",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "connector": {"id": "conn-new", "name": "New Connector"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("connector_create");
    fs::write(&file_path, r#"{"name": "New Connector"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "notifications",
            "connectors",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/notification-center/connectors?id=conn-new"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn connector_update_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/connectors",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "connector": {"id": "conn-1", "name": "Updated Connector"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("connector_update");
    fs::write(
        &file_path,
        r#"{"id": "conn-1", "name": "Updated Connector"}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "notifications",
            "connectors",
            "update",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/notification-center/connectors?id=conn-1"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

/// `notifications connectors list` has a distinct console URL per connector
/// (same as `alerts list`), so every row must be tagged with its own
/// `consoleUrl` - not just the first.
#[tokio::test]
async fn connectors_list_tags_every_row_with_its_own_console_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/connectors",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "connectors": [
                {"id": "conn-1", "name": "First Connector"},
                {"id": "conn-2", "name": "Second Connector"},
                {"id": "conn-3", "name": "Third Connector"}
            ]
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "notifications",
            "connectors",
            "list",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stdout: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let items = stdout.as_array().expect("expected a json array");
    assert_eq!(items.len(), 3);
    for (i, id) in ["conn-1", "conn-2", "conn-3"].iter().enumerate() {
        assert_eq!(
            items[i]["consoleUrl"],
            format!("https://c4c.app.eu2.coralogix.com/notification-center/connectors?id={id}"),
            "row {i} did not carry its own connector's consoleUrl: {items:#?}"
        );
    }

    // Text mode doesn't repeat every connector's own link in the table (that
    // would bloat it) - instead it gets a single "View in Coralogix" line
    // to stderr pointing at the connectors list page itself.
    let text_output = cx(&home)
        .args([
            "--profile",
            "mock",
            "notifications",
            "connectors",
            "list",
            "-o",
            "text",
        ])
        .output()
        .expect("failed to run cx");
    assert!(text_output.status.success(), "{:?}", text_output);
    let stdout = String::from_utf8_lossy(&text_output.stdout);
    assert!(
        !stdout.contains("Console URL"),
        "text table should not have a per-row Console URL column: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&text_output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/notification-center/connectors"
        ),
        "stderr did not contain the connectors list page link: {stderr}"
    );
}

// ── notifications routers create/update ──────────────────────────────────────

#[tokio::test]
async fn router_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/routers",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "router": {"id": "router-new", "name": "New Router"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("router_create");
    fs::write(&file_path, r#"{"name": "New Router"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "notifications",
            "routers",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/notification-center/routers?id=router-new"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn router_update_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/routers",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "router": {"id": "router-1", "name": "Updated Router"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("router_update");
    fs::write(
        &file_path,
        r#"{"id": "router-1", "name": "Updated Router"}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "notifications",
            "routers",
            "update",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/notification-center/routers?id=router-1"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── iam roles create/update ──────────────────────────────────────────────────

#[tokio::test]
async fn iam_role_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/aaa/custom-roles/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "role-new"})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("iam_role_create");
    fs::write(&file_path, r#"{"name": "New Role"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "iam",
            "roles",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/roles?selectedRoleId=role-new"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn iam_role_update_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/aaa/custom-roles/v1/role-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "role-1"})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("iam_role_update");
    fs::write(&file_path, r#"{"name": "Updated Role"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "iam",
            "roles",
            "update",
            "role-1",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/roles?selectedRoleId=role-1"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── iam scopes create/update ─────────────────────────────────────────────────

#[tokio::test]
async fn iam_scope_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/aaa/team-scopes/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "scope": {"id": "scope-new", "displayName": "New Scope"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("iam_scope_create");
    fs::write(&file_path, r#"{"displayName": "New Scope"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "iam",
            "scopes",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/scopes?selectedScopeId=scope-new"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn iam_scope_update_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/aaa/team-scopes/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "scope-1", "displayName": "Updated Scope"
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("iam_scope_update");
    fs::write(
        &file_path,
        r#"{"id": "scope-1", "displayName": "Updated Scope"}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "iam",
            "scopes",
            "update",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/scopes?selectedScopeId=scope-1"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── iam groups create/update ─────────────────────────────────────────────────

#[tokio::test]
async fn iam_group_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/aaa/team-groups/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "group": {"groupId": "group-new", "name": "New Group"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("iam_group_create");
    fs::write(&file_path, r#"{"name": "New Group"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "iam",
            "groups",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/account/groups?selectedGroupId=group-new"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn iam_group_update_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/aaa/team-groups/v2/group-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "group": {"groupId": "group-1", "name": "Updated Group"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("iam_group_update");
    fs::write(&file_path, r#"{"name": "Updated Group"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "iam",
            "groups",
            "update",
            "group-1",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/account/groups?selectedGroupId=group-1"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── Static, per-feature links (no per-entity ID) ────────────────────────────
//
// These groups map to a single settings/report page in the console rather
// than a per-entity route - per reviewer feedback on FORGE-586, "an entity
// was created/updated" was never the actual bar for adding a link, just the
// easiest example. `cx usage` (100% read-only) is the reviewer's own example
// of a group that should still get a link because a real page exists for it.

#[tokio::test]
async fn usage_summary_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataplans/data-usage/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"usage": {"totalGb": 42.5}})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "usage", "summary"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/datausage"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn tco_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dataplans/policies/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "policy": {"id": "tco-1", "name": "New Policy"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("tco_create");
    fs::write(&file_path, r#"{"name": "New Policy"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "tco",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/tco-policies"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn enrichments_add_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/enrichment-rules/enrichment-rules/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"enrichments": []})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("enrichments_add");
    fs::write(
        &file_path,
        r#"{"requestEnrichments": [{"fieldName": "sourceIPs", "enrichmentType": {"geoIp": {}}}]}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "enrichments",
            "add",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/enrichments"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn enrichments_custom_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/mgmt/openapi/5/enrichment-rules/custom-enrichment-rules/v1",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "customEnrichment": {"id": "ce-1", "name": "IP Lookup"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("enrichments_custom_create");
    fs::write(
        &file_path,
        r#"{
            "name": "IP Lookup",
            "description": "Maps IPs to locations",
            "file": {
                "textual": "ip,city\n1.2.3.4,London",
                "extension": "csv",
                "name": "lookup.csv",
                "size": 24
            }
        }"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "enrichments",
            "custom",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/enrichments"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn integrations_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/integrations/integrations/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "deployment": {"id": "int-1", "name": "New Integration"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("integrations_create");
    fs::write(&file_path, r#"{"name": "New Integration"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "integrations",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/extensions/integrations"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

// NOTE: `webhooks create` console-link coverage lives in the FORGE-696 follow-up
// PR, which fixes the underlying bug that makes the link reachable at all. See
// the "webhooks create" entry in verification/forge-586-console-links/.

#[tokio::test]
async fn iam_api_keys_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/aaa/api-keys/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keyId": "key-1", "name": "New Key", "value": "secret-value"
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("iam_api_keys_create");
    fs::write(&file_path, r#"{"name": "New Key"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "iam",
            "api-keys",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/api-keys"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn iam_ip_access_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/aaa/team-sec-ip-access/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "settings": {"id": "ip-1", "ipAccess": []}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("iam_ip_access_create");
    fs::write(&file_path, r#"{"ipAccess": []}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "iam",
            "ip-access",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/login-access-policies"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn iam_users_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/identity/whoami"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"team_id": 123})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/aaa/teams/v2/123/members"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("iam_users_create");
    fs::write(
        &file_path,
        r#"{"users": [{"userName": "new.user@c4c.com"}]}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "iam",
            "users",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);

    // whoami IS expected here (unlike the other tests) - it resolves the
    // team ID needed for the `{USERS_BASE}/{team_id}/members` request path,
    // which is unrelated to (and independent of) the console link itself.
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .any(|r| r.url.path() == "/identity/whoami"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr
            .contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/team/members"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn ai_center_applications_get_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/ai/applications/v3/app-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "aiApplication": {
                "id": "app-1", "name": "My Application",
                "application": "checkout", "subsystem": "payments"
            }
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "ai-center",
            "applications",
            "get",
            "app-1",
        ])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/ai-center/application/drilldown?application=checkout&subsystem=payments"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn ai_center_applications_get_falls_back_to_catalog_without_application_field() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/ai/applications/v3/app-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "aiApplication": { "id": "app-1", "name": "My Application" }
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "ai-center",
            "applications",
            "get",
            "app-1",
        ])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/ai-center/application-catalog"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn ai_center_evaluations_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/ai/evaluations/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "eval-1", "name": "New Evaluation"
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("ai_center_evaluations_create");
    fs::write(&file_path, r#"{"name": "New Evaluation"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "ai-center",
            "evaluations",
            "create",
            "--from-file",
            file_path.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/ai-center/eval-catalog"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn olly_ask_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-123/interactions/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-1",
            "chat_id": "chat-123",
            "status": "COMPLETED",
            "responses": [
                {
                    "id": "msg-1",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Here's the answer."}]
                }
            ]
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "olly",
            "ask",
            "What alerts fired today?",
            "--chat-id",
            "chat-123",
        ])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/olly"),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── Consistency pass: static-page groups also link on their read-only
// subcommands, not just writes ─────────────────────────────────────────
//
// The `usage` group already established that a purely read-only command can
// still print a link when a real page exists for it. These groups had that
// same static page, but (inconsistently) only linked their mutation
// subcommands. A handful of representative reads are covered here; the full
// read+write wiring lives in each group's `mod.rs`.

#[tokio::test]
async fn tco_list_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataplans/policies/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "policies": [{"id": "policy-1", "name": "Demo Policy"}]
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "tco", "list"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/tco-policies"),
        "stderr did not contain the console link: {stderr}"
    );
}

/// Regression test for a real-world bug: when a team has zero TCO policies,
/// the list command used to print the console link to stderr unconditionally
/// while `-o json`/`-o agents` had no row left to tag it onto - violating the
/// "stderr and consoleUrl never disagree" invariant. Resolving (and
/// printing) the link must be skipped entirely when the list is empty.
#[tokio::test]
async fn tco_list_empty_prints_no_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dataplans/policies/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"policies": []})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "tco", "list"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("View in Coralogix:"),
        "stderr unexpectedly contained a console link for an empty policy list: {stderr}"
    );
}

#[tokio::test]
async fn ip_access_get_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/team-sec-ip-access/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ipAccess": []})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "iam", "ip-access", "get"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/settings/login-access-policies"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn ai_center_evaluations_list_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/ai/evaluations/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "aiEvaluations": [{"id": "eval-1", "application": "app1", "subsystem": "sub1"}]
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "ai-center", "evaluations", "list"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/ai-center/eval-catalog"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

/// Regression test for a real-world bug: when a team has zero AI Center
/// evaluations, the list command used to print the console link to stderr
/// unconditionally while `-o json`/`-o agents` had no row left to tag it
/// onto - violating the "stderr and consoleUrl never disagree" invariant.
/// Resolving (and printing) the link must be skipped entirely when the list
/// is empty.
#[tokio::test]
async fn ai_center_evaluations_list_empty_prints_no_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/ai/evaluations/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"aiEvaluations": []})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "ai-center", "evaluations", "list"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("View in Coralogix:"),
        "stderr unexpectedly contained a console link for an empty evaluations list: {stderr}"
    );
}

/// Regression test: unlike other list commands (whose console link is one
/// static per-profile "page" URL, not any individual row's own link),
/// `alerts list` has a distinct console URL per alert, so every row must be
/// tagged with its own `consoleUrl` - not just the first.
#[tokio::test]
async fn alerts_list_tags_every_row_with_its_own_console_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertDefs": [
                {"id": "alert-1", "name": "First Alert"},
                {"id": "alert-2", "name": "Second Alert"},
                {"id": "alert-3", "name": "Third Alert"}
            ]
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "alerts", "list", "-o", "json"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stdout: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let items = stdout.as_array().expect("expected a json array");
    assert_eq!(items.len(), 3);
    for (i, id) in ["alert-1", "alert-2", "alert-3"].iter().enumerate() {
        assert_eq!(
            items[i]["consoleUrl"],
            format!("https://c4c.app.eu2.coralogix.com/alerts/{id}"),
            "row {i} did not carry its own alert's consoleUrl: {items:#?}"
        );
    }

    // Text mode doesn't repeat every alert's own link in the table (that
    // would bloat it) - instead it gets a single "View in Coralogix" line
    // to stderr pointing at the alerts list page itself.
    let text_output = cx(&home)
        .args(["--profile", "mock", "alerts", "list", "-o", "text"])
        .output()
        .expect("failed to run cx");
    assert!(text_output.status.success(), "{:?}", text_output);
    let stdout = String::from_utf8_lossy(&text_output.stdout);
    assert!(
        !stdout.contains("Console URL"),
        "text table should not have a per-row Console URL column: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&text_output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/alerts"),
        "stderr did not contain the alerts list page link: {stderr}"
    );
}

// ── get/enable/disable on known-id routes (FORGE-586 follow-up) ─────────────
//
// These cover the previously-unwired `get`/`enable`/`disable`/`check`
// subcommands that share the exact same console route as their sibling
// `create`/`update` commands (already covered above), since the entity id
// is already known from the CLI argument.

#[tokio::test]
async fn dashboard_get_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1/dash-abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "dash-abc123"})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "dashboards", "get", "dash-abc123"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn dashboard_check_by_id_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/check/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": []})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "dashboards", "check", "dash-abc123"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

/// `-o json` on `dashboards check <id>` must embed the same URL as a
/// `consoleUrl` field on every issue row (there's no single "the dashboard"
/// object here - `check` returns a list of validation issues - so the URL is
/// repeated per row via `issue_json_row`'s `console_url` parameter).
#[tokio::test]
async fn dashboard_check_by_id_json_output_includes_console_url_on_first_issue_only() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/check/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issues": [
                {"severity": "SEVERITY_WARNING", "message": "deprecated function", "location": "/a"},
                {"severity": "SEVERITY_ERROR", "message": "bad query", "location": "/b"},
            ]
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "dashboards",
            "check",
            "dash-abc123",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to run cx");

    // Exits non-zero because of the error-severity issue - CI-gate semantics -
    // but stdout should still have rendered the JSON rows first.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nstdout: {stdout}"));
    let rows = parsed.as_array().expect("expected a JSON array of issues");
    assert_eq!(rows.len(), 2);
    // It's one static dashboard-page link for the whole check, not per-issue -
    // only the first row should carry it so `-o agents` output doesn't repeat
    // the identical URL once per issue.
    assert_eq!(
        rows[0].get("consoleUrl").and_then(|v| v.as_str()),
        Some("https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123"),
        "expected consoleUrl on the first issue row: {}",
        rows[0]
    );
    for row in &rows[1..] {
        // The key stays present-but-null (rather than absent) so that `-o
        // agents` TOON-encodes the array in its compact tabular form, which
        // requires every row to share the same key set.
        assert_eq!(
            row.get("consoleUrl"),
            Some(&serde_json::Value::Null),
            "expected consoleUrl to be null (not absent) on subsequent issue rows: {row}"
        );
    }
}

#[tokio::test]
async fn dashboard_check_from_file_prints_no_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/check/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": []})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("dash_check_file");
    fs::write(
        &file_path,
        r#"{"name": "Demo Dashboard", "layout": {"sections": []}}"#,
    )
    .unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "dashboards",
            "check",
            "--from-file",
            file_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run cx");

    let _ = fs::remove_file(&file_path);
    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("View in Coralogix:"),
        "stderr unexpectedly contained a console link when checking from a file: {stderr}"
    );
}

#[tokio::test]
async fn alert_get_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/alert-xyz789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertDef": {"id": "alert-xyz789", "name": "Demo Alert"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "alerts", "get", "alert-xyz789"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/alerts/alert-xyz789"),
        "stderr did not contain the console link: {stderr}"
    );
}

/// `-o json` on `alerts get` must embed the same URL as a `consoleUrl` field
/// nested inside the `alertDef` wrapper object (see
/// `render::tag_console_url`'s single-object-wrapper nesting), not at the
/// JSON root, and not only printed to stderr.
#[tokio::test]
async fn alert_get_json_output_includes_console_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/alert-xyz789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertDef": {"id": "alert-xyz789", "name": "Demo Alert"}
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "alerts",
            "get",
            "alert-xyz789",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(
        parsed.get("consoleUrl"),
        None,
        "consoleUrl must not sit at the root alongside alertDef: {parsed}"
    );
    assert_eq!(
        parsed
            .get("alertDef")
            .and_then(|v| v.get("consoleUrl"))
            .and_then(|v| v.as_str()),
        Some("https://c4c.app.eu2.coralogix.com/alerts/alert-xyz789"),
        "expected consoleUrl field nested inside alertDef: {parsed}"
    );
}

#[tokio::test]
async fn alert_enable_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/alert-xyz789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertDef": {
                "id": "alert-xyz789",
                "alertDefProperties": {"name": "Demo Alert", "enabled": false}
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "alerts",
            "enable",
            "alert-xyz789",
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/alerts/alert-xyz789"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn alert_disable_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3/alert-xyz789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertDef": {
                "id": "alert-xyz789",
                "alertDefProperties": {"name": "Demo Alert", "enabled": true}
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "alerts",
            "disable",
            "alert-xyz789",
            "--yes",
        ])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/alerts/alert-xyz789"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn case_get_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/cases/cases/v1/case-777"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "case": {
                "id": "case-777",
                "readableId": "CASE-42",
                "title": "Checkout errors",
                "status": "CASE_STATUS_OPEN",
                "priority": "CASE_PRIORITY_P2"
            }
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "cases", "get", "case-777"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://c4c.app.eu2.coralogix.com/cases?id=case-777"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn connector_get_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/notifications/notification-center/v1/connectors/conn-1",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "conn-1",
            "name": "Demo Connector",
            "type": "CONNECTOR_TYPE_SLACK"
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(
        &home,
        "mock",
        &server.uri(),
        Some("https://c4c.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "notifications",
            "connectors",
            "get",
            "conn-1",
        ])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://c4c.app.eu2.coralogix.com/notification-center/connectors?id=conn-1"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}
