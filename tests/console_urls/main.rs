//! Integration tests for "View in Coralogix" console links (FORGE-586).
//!
//! After a successful `dashboards create`/`replace`, `alerts create`, or
//! `cases` lifecycle mutation, the CLI should print a
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/dashboards/dash-abc123"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/dashboards/dash-abc123"
        ),
        "stderr did not contain the console link: {stderr}"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr
            .contains("View in Coralogix: https://acme.app.eu2.coralogix.com/alerts/alert-xyz789"),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr.contains("View in Coralogix: https://acme.app.eu2.coralogix.com/cases?id=case-777"),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── No console domain / no override -> no link, command still succeeds ──────

#[tokio::test]
async fn no_console_link_when_region_has_no_known_console_domain() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"dashboardId": "dash-abc123"})),
        )
        .mount(&server)
        .await;

    let home = temp_home();
    // No console_url override; `region` is a bare custom URL (Region::Custom),
    // which has no known console domain - console_base must resolve to None.
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
        "unexpected console link with no known console domain: {stderr}"
    );
    assert!(
        stderr.contains("Created dashboard 'Demo Dashboard' (ID: dash-abc123)"),
        "expected the usual success line, stderr: {stderr}"
    );
}

// ── stdout is unaffected by the console link ─────────────────────────────────

#[tokio::test]
async fn json_output_is_unaffected_by_console_link() {
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
        Some("https://acme.app.eu2.coralogix.com"),
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

    // stdout must be exactly the API response - no console link text anywhere.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(parsed, json!({"dashboardId": "dash-abc123"}));
    assert!(!stdout.contains("View in Coralogix"));

    // The link still goes to stderr.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr
        .contains("View in Coralogix: https://acme.app.eu2.coralogix.com/dashboards/dash-abc123"));
}
