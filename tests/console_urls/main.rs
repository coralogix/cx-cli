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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/dashboards/dash-abc123"
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/dashboards/dash-abc123"
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
        stderr.contains(
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/alerts/alert-xyz789"
        ),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/explore?viewId=view-123"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/explore?viewId=view-123"
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
        stderr
            .contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/cases?id=case-777"),
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
    assert!(stderr.contains(
        "View in Coralogix: https://acme.app.eu2.coralogix.com/#/dashboards/dash-abc123"
    ));
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr.contains(
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/tco/metrics/e2m-new"
        ),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr
            .contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/tco/metrics/e2m-1"),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr.contains(
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/slo/slo-new/overview"
        ),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr
            .contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/slo/slo-1/overview"),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── parsing-rules create/update ──────────────────────────────────────────────

#[tokio::test]
async fn parsing_rule_group_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/parsing-rules/rule-groups/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ruleGroup": {"id": "rg-new", "name": "New Rule Group"}
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

    let file_path = temp_json_path("rule_group_create");
    fs::write(&file_path, r#"{"name": "New Rule Group"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "parsing-rules",
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
            .contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/rules/group/rg-new"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn parsing_rule_group_update_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/parsing-rules/rule-groups/v1/rg-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "rg-1", "name": "Updated"})),
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

    let file_path = temp_json_path("rule_group_update");
    fs::write(&file_path, r#"{"name": "Updated"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "parsing-rules",
            "update",
            "rg-1",
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
        stderr.contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/rules/group/rg-1"),
        "stderr did not contain the console link: {stderr}"
    );
}

// ── alerts suppression-rules create/update ───────────────────────────────────

#[tokio::test]
async fn suppression_rule_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/alerts/suppression-rules/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertSchedulerRule": {"id": "rule-new", "name": "New Rule"}
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

    let file_path = temp_json_path("suppression_rule_create");
    fs::write(&file_path, r#"{"name": "New Rule"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "alerts",
            "suppression-rules",
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/suppression-rules?edit=rule-new"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn suppression_rule_update_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/alerts/suppression-rules/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "alertSchedulerRule": {"id": "rule-1", "name": "Updated Rule"}
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

    let file_path = temp_json_path("suppression_rule_update");
    fs::write(&file_path, r#"{"id": "rule-1", "name": "Updated Rule"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "alerts",
            "suppression-rules",
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/suppression-rules?edit=rule-1"
        ),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/notification-center/connectors?id=conn-new"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/notification-center/connectors?id=conn-1"
        ),
        "stderr did not contain the console link: {stderr}"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/notification-center/routers?id=router-new"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/notification-center/routers?id=router-1"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/roles?selectedRoleId=role-new"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/roles?selectedRoleId=role-1"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/scopes?selectedScopeId=scope-new"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/scopes?selectedScopeId=scope-1"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/account/groups?selectedGroupId=group-new"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/account/groups?selectedGroupId=group-1"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}
