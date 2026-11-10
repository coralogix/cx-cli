//! Integration tests for the update notice (HEA-62): the `_meta.update`
//! contract in `cx schema` and toon output, and the plain stderr notice.
//!
//! Each test seeds `~/.cx/state.json` in a temp home. `last_checked_at` is set
//! to now so the background version fetch stays off the network, and
//! `latest_binary` equals the running version so only the skills check can
//! produce a notice.

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

static COUNTER: AtomicU32 = AtomicU32::new(0);

const CURRENT: &str = env!("CARGO_PKG_VERSION");

fn temp_home(tag: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cx_test_update_notice_{tag}_{pid}_{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".cx")).unwrap();
    dir
}

/// Write `state.json` with `extra` merged into the fields every test needs.
fn write_state(home: &Path, extra: Value) {
    let mut state = serde_json::json!({
        "last_checked_at": chrono::Utc::now().to_rfc3339(),
        "latest_binary": CURRENT,
    });
    for (k, v) in extra.as_object().unwrap() {
        state[k] = v.clone();
    }
    fs::write(home.join(".cx").join("state.json"), state.to_string()).unwrap();
}

fn read_state(home: &Path) -> Value {
    let contents = fs::read_to_string(home.join(".cx").join("state.json")).unwrap();
    serde_json::from_str(&contents).unwrap()
}

/// Write an installer lock file that lists one cx skill, updated long before
/// this binary was built.
fn write_old_lock(home: &Path) {
    let dir = home.join(".agents");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(".skill-lock.json"),
        r#"{"version":3,"skills":{"cx-cli":{"source":"coralogix/cx-cli",
            "installedAt":"2020-01-01T00:00:00.000Z","updatedAt":"2020-01-01T00:00:00.000Z"}}}"#,
    )
    .unwrap();
}

/// A `cx` command sealed off from the developer's real home and environment.
fn cx(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("HOME", home)
        .env("CX_HOME", home)
        .env_remove("CX_NO_UPDATE_NOTIFIER")
        .env_remove("XDG_STATE_HOME")
        .env_remove("CX_API_KEY")
        .env_remove("CX_REGION")
        .env_remove("CX_PROFILE");
    cmd
}

fn schema_meta(home: &Path) -> Value {
    let output = cx(home).arg("schema").output().expect("failed to run cx");
    assert!(output.status.success(), "cx schema failed");
    let schema: Value = serde_json::from_slice(&output.stdout).expect("schema is JSON");
    schema["_meta"].clone()
}

// ── cx schema ─────────────────────────────────────────────────────────────────

#[test]
fn schema_meta_has_skills_only_update_when_skills_outdated() {
    let home = temp_home("schema_outdated");
    write_state(
        &home,
        serde_json::json!({ "skills_installed_for": "0.0.1" }),
    );

    let meta = schema_meta(&home);
    let update = &meta["update"];
    assert!(update.get("binary").is_none(), "no binary update: {meta}");
    assert_eq!(update["skills"]["reason"], "outdated", "{meta}");
    assert_eq!(update["skills"]["installed_for"], "0.0.1", "{meta}");
    assert_eq!(
        update["skills"]["command"], "cx skills install --global",
        "{meta}"
    );
}

#[test]
fn schema_meta_absent_when_skills_current() {
    let home = temp_home("schema_current");
    write_state(
        &home,
        serde_json::json!({ "skills_installed_for": CURRENT }),
    );

    let meta = schema_meta(&home);
    assert!(meta.is_null(), "expected no _meta: {meta}");
}

#[test]
fn schema_meta_absent_when_no_cx_skills_found() {
    let home = temp_home("schema_none");
    write_state(&home, serde_json::json!({}));

    let meta = schema_meta(&home);
    assert!(meta.is_null(), "expected no _meta: {meta}");
}

// ── toon trailing line ────────────────────────────────────────────────────────

#[tokio::test]
async fn toon_output_ends_with_skills_only_update() {
    // The command's own result does not matter: the `_meta` line prints after
    // the command, even when it fails.
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let home = temp_home("toon_outdated");
    write_state(
        &home,
        serde_json::json!({ "skills_installed_for": "0.0.1" }),
    );
    let output = cx(&home)
        .args([
            "alerts",
            "list",
            "-o",
            "toon",
            "--api-key",
            "test-key",
            "--region",
            &server.uri(),
        ])
        .output()
        .expect("failed to run cx");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.lines().last().unwrap_or_default();
    let line: Value = serde_json::from_str(last)
        .unwrap_or_else(|e| panic!("last stdout line is not JSON ({e}): {stdout}"));
    let update = &line["_meta"]["update"];
    assert!(update.get("binary").is_none(), "no binary update: {line}");
    assert_eq!(update["skills"]["reason"], "outdated", "{line}");
    assert_eq!(update["skills"]["installed_for"], "0.0.1", "{line}");
}

// ── stderr notice ─────────────────────────────────────────────────────────────

#[test]
fn unknown_version_notice_shows_once_per_cx_version() {
    let home = temp_home("unknown_once");
    write_state(&home, serde_json::json!({}));
    write_old_lock(&home);

    let first = cx(&home).arg("cleanup").output().expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&first.stderr);
    assert!(
        stderr.contains("[cx update] cx can't tell which version your cx agent skills are")
            && stderr.contains("skills: cx skills install --global"),
        "expected unknown-version notice, stderr: {stderr}"
    );
    assert_eq!(read_state(&home)["skills_unknown_noticed_for"], CURRENT);

    let second = cx(&home).arg("cleanup").output().expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        !stderr.contains("[cx update]"),
        "notice must show only once per version, stderr: {stderr}"
    );
}

#[test]
fn notice_silenced_by_env_var() {
    let home = temp_home("silenced");
    write_state(
        &home,
        serde_json::json!({ "skills_installed_for": "0.0.1" }),
    );

    let output = cx(&home)
        .env("CX_NO_UPDATE_NOTIFIER", "1")
        .arg("cleanup")
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("[cx update]"), "stderr: {stderr}");
}
