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
    // "View in Coralogix:" (with the colon) is the actual link line's prefix
    // everywhere else in this file - checked instead of the bare phrase
    // "View in Coralogix" since the no-link hint below legitimately names the
    // feature without emitting a link.
    assert!(
        !stderr.contains("View in Coralogix:"),
        "unexpected console link with no known console domain: {stderr}"
    );
    assert!(
        stderr.contains("Created dashboard 'Demo Dashboard' (ID: dash-abc123)"),
        "expected the usual success line, stderr: {stderr}"
    );
    assert!(
        stderr.contains("no \"View in Coralogix\" link available"),
        "expected the no-link hint explaining why, stderr: {stderr}"
    );
}

/// An explicit `console_team_name` is parsed and threaded through, but a
/// `Region::Custom` profile (a bare mock URL, same as the test above) still
/// has no *known console domain* to combine it with - so setting the field
/// alone must not produce a link here. This guards against `console_base`
/// resolution accidentally short-circuiting the "no known domain" branch.
/// The full "known domain + explicit console_team_name" combination is
/// covered at the unit level in
/// `execution::tests::console_base_combines_domain_and_explicit_team_name`,
/// since the region enum used by these binary-level tests has no way to
/// point a *known* region's API base at a wiremock server (see
/// `src/config.rs`'s `Region::api_endpoint`/`console_domain`).
///
/// There is no `/identity/whoami` mock in this test at all: with no known
/// console domain, `console_base` returns `None` before ever considering
/// `console_team_name` or falling back to `/identity/whoami`, so no API call
/// is attempted here regardless of the field being set. (When a console
/// domain *is* known, an explicit `console_team_name` still skips
/// `/identity/whoami` - see
/// `execution::tests::console_base_explicit_team_name_takes_precedence_over_whoami`
/// - but by default, with neither `console_url` nor `console_team_name` set,
/// resolving a console link does make that API call now; see
/// `execution::tests::console_base_combines_domain_and_team_subdomain_from_whoami`.)
#[tokio::test]
async fn console_team_name_does_not_produce_link_without_known_console_domain() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/dashboards/dashboards/v1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"dashboardId": "dash-abc123"})),
        )
        .mount(&server)
        .await;

    let home = temp_home();
    let profiles_dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    fs::write(
        profiles_dir.join("mock.toml"),
        format!(
            r#"auth = "api_key"
credential_storage = "file"
api_key = "test-key"
region = "{}"
console_team_name = "acme"
"#,
            server.uri()
        ),
    )
    .unwrap();
    write_config(&home, "mock");

    let file_path = temp_json_path("dash_team_name_no_domain");
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
        !stderr.contains("View in Coralogix:"),
        "unexpected console link with no known console domain: {stderr}"
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

    // stdout is the API response plus a `consoleUrl` field - no
    // "View in Coralogix: " prefix text (that's stderr-only phrasing).
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(
        parsed,
        json!({
            "dashboardId": "dash-abc123",
            "consoleUrl": "https://acme.app.eu2.coralogix.com/#/dashboards/dash-abc123",
        })
    );
    assert!(!stdout.contains("View in Coralogix"));

    // The human-readable line still goes to stderr too, with the same URL.
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
        Some("https://acme.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "usage", "summary"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr
            .contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/datausage"),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr.contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/tco-policies"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn archive_metrics_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/metrics/data-setup/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"enabled": true})))
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

    let file_path = temp_json_path("archive_metrics_create");
    fs::write(&file_path, r#"{"enabled": true}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "archive",
            "metrics",
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
            .contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/physical-locations"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn recording_rules_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/recording-rules/recording-rules/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "group": {"id": "rr-1", "name": "New Group"}
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

    let file_path = temp_json_path("recording_rules_create");
    fs::write(&file_path, r#"{"name": "New Group"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "recording-rules",
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
        stderr.contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/recording-rules"),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr.contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/enrichments"),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr.contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/enrichments"),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/extensions/integrations"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn webhooks_create_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/openapi/5/integrations/webhooks/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "webhook": {"id": "wh-1", "name": "New Webhook"}
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

    let file_path = temp_json_path("webhooks_create");
    fs::write(&file_path, r#"{"name": "New Webhook"}"#).unwrap();

    let output = cx(&home)
        .args([
            "--profile",
            "mock",
            "webhooks",
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/extensions/outbound-webhooks"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr
            .contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/api-keys"),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/login-access-policies"
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
        Some("https://acme.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let file_path = temp_json_path("iam_users_create");
    fs::write(
        &file_path,
        r#"{"users": [{"userName": "new.user@acme.com"}]}"#,
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
        stderr.contains(
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/team/members"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn ai_center_applications_get_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/ai/applications/v3/app-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "app-1", "name": "My Application"
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/ai-center/overview/application-catalog"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/ai-center/overview/eval-catalog"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr.contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/olly"),
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
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"policies": []})))
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
        .args(["--profile", "mock", "tco", "list"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/tco-policies"),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn archive_metrics_get_prints_console_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/metrics/data-setup/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"enabled": true})))
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
        .args(["--profile", "mock", "archive", "metrics", "get"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr
            .contains("View in Coralogix: https://acme.app.eu2.coralogix.com/#/physical-locations"),
        "stderr did not contain the console link: {stderr}"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/settings/login-access-policies"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

#[tokio::test]
async fn ai_center_evaluations_list_prints_console_link() {
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/ai-center/overview/eval-catalog"
        ),
        "stderr did not contain the console link: {stderr}"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/dashboards/dash-abc123"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/dashboards/dash-abc123"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        Some("https://acme.app.eu2.coralogix.com/#/dashboards/dash-abc123"),
        "expected consoleUrl on the first issue row: {}",
        rows[0]
    );
    for row in &rows[1..] {
        assert!(
            row.get("consoleUrl").is_none(),
            "expected no consoleUrl on subsequent issue rows: {row}"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        Some("https://acme.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "alerts", "get", "alert-xyz789"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/alerts/alert-xyz789"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}

/// `-o json` on `alerts get` must embed the same URL as a `consoleUrl` field
/// on the returned alert object (see `render::tag_console_url`), not only
/// print it to stderr.
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        parsed.get("consoleUrl").and_then(|v| v.as_str()),
        Some("https://acme.app.eu2.coralogix.com/#/alerts/alert-xyz789"),
        "expected consoleUrl field in JSON output: {parsed}"
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr.contains(
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/alerts/alert-xyz789"
        ),
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
        Some("https://acme.app.eu2.coralogix.com"),
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
        stderr.contains(
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/alerts/alert-xyz789"
        ),
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
        Some("https://acme.app.eu2.coralogix.com"),
    );
    write_config(&home, "mock");

    let output = cx(&home)
        .args(["--profile", "mock", "cases", "get", "case-777"])
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
        Some("https://acme.app.eu2.coralogix.com"),
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
            "View in Coralogix: https://acme.app.eu2.coralogix.com/#/notification-center/connectors?id=conn-1"
        ),
        "stderr did not contain the console link: {stderr}"
    );
}
