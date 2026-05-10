use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use assert_cmd::Command;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_home() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cx_profile_test_{pid}_{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_profile(home: &std::path::Path, name: &str, api_key: &str, region: &str) {
    let profiles_dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    let content = format!(
        r#"auth = "api_key"
credential_storage = "file"
api_key = "{api_key}"
region = "{region}"
"#
    );
    fs::write(profiles_dir.join(format!("{name}.toml")), content).unwrap();
}

fn write_config(home: &std::path::Path, default_profile: &str) {
    let cx_dir = home.join(".cx");
    fs::create_dir_all(&cx_dir).unwrap();
    let content = format!("default_profile = \"{default_profile}\"\n");
    fs::write(cx_dir.join("config.toml"), content).unwrap();
}

fn cx(home: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("CX_HOME", home);
    cmd.env_remove("CX_API_KEY");
    cmd.env_remove("CX_REGION");
    cmd.env_remove("CX_PROFILE");
    cmd
}

// ── --profile suppresses CX_API_KEY env var ──────────────────────────────────

#[tokio::test]
async fn explicit_profile_uses_profile_key_not_env_var() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/alerts/alerts-general/v3"))
        .and(header("Authorization", "Bearer profile-key-aaa"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "alertDefs": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(&home, "myprof", "profile-key-aaa", &server.uri());
    write_config(&home, "myprof");

    cx(&home)
        .env("CX_API_KEY", "env-key-should-be-ignored")
        .args(["--profile", "myprof", "alerts", "list"])
        .assert()
        .success();
}

#[tokio::test]
async fn no_explicit_profile_uses_env_var_key() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/alerts/alerts-general/v3"))
        .and(header("Authorization", "Bearer env-key-bbb"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "alertDefs": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(&home, "default", "profile-key-ignored", &server.uri());
    write_config(&home, "default");

    cx(&home)
        .env("CX_API_KEY", "env-key-bbb")
        .args(["alerts", "list"])
        .assert()
        .success();
}

#[tokio::test]
async fn explicit_api_key_flag_overrides_profile_key() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/alerts/alerts-general/v3"))
        .and(header("Authorization", "Bearer explicit-flag-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "alertDefs": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(&home, "myprof", "profile-key-ignored", &server.uri());
    write_config(&home, "myprof");

    cx(&home)
        .args([
            "--profile",
            "myprof",
            "--api-key",
            "explicit-flag-key",
            "alerts",
            "list",
        ])
        .assert()
        .success();
}

// ── --profile suppresses CX_REGION env var ───────────────────────────────────

#[tokio::test]
async fn explicit_profile_uses_profile_region_not_env_var() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/alerts/alerts-general/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "alertDefs": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(&home, "myprof", "key", &server.uri());
    write_config(&home, "myprof");

    // CX_REGION points to a different region; --profile should ignore it and
    // use the profile's region (the mock server URI).
    cx(&home)
        .env("CX_REGION", "eu1")
        .args(["--profile", "myprof", "alerts", "list"])
        .assert()
        .success();
}

// ── Multi-profile + env var does NOT trigger the guard ───────────────────────

#[tokio::test]
async fn multi_profile_with_env_api_key_does_not_bail() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/alerts/alerts-general/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "alertDefs": []
        })))
        .expect(2)
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(&home, "prof_a", "key-a", &server.uri());
    write_profile(&home, "prof_b", "key-b", &server.uri());
    write_config(&home, "prof_a");

    let output = cx(&home)
        .env("CX_API_KEY", "env-key-should-be-ignored")
        .args([
            "--profile",
            "prof_a",
            "--profile",
            "prof_b",
            "alerts",
            "list",
        ])
        .output()
        .expect("failed to run cx");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Cannot combine"),
        "CX_API_KEY env var should not trigger the multi-profile guard, stderr: {stderr}"
    );
}

#[tokio::test]
async fn multi_profile_with_env_region_does_not_bail() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/latest/alerts/alerts-general/v3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "alertDefs": []
        })))
        .expect(2)
        .mount(&server)
        .await;

    let home = temp_home();
    write_profile(&home, "prof_a", "key-a", &server.uri());
    write_profile(&home, "prof_b", "key-b", &server.uri());
    write_config(&home, "prof_a");

    let output = cx(&home)
        .env("CX_REGION", "us2")
        .args([
            "--profile",
            "prof_a",
            "--profile",
            "prof_b",
            "alerts",
            "list",
        ])
        .output()
        .expect("failed to run cx");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Cannot combine"),
        "CX_REGION env var should not trigger the multi-profile guard, stderr: {stderr}"
    );
}

// ── Explicit --api-key / --region with multiple profiles STILL rejected ──────

#[test]
fn multi_profile_with_explicit_api_key_flag_still_rejected() {
    let home = temp_home();
    write_profile(&home, "prof_a", "key-a", "us1");
    write_profile(&home, "prof_b", "key-b", "us1");
    write_config(&home, "prof_a");

    let output = cx(&home)
        .args([
            "--profile",
            "prof_a",
            "--profile",
            "prof_b",
            "--api-key",
            "explicit-key",
            "alerts",
            "list",
        ])
        .output()
        .expect("failed to run cx");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Cannot combine"),
        "explicit --api-key with multiple profiles should be rejected, stderr: {stderr}"
    );
}

#[test]
fn multi_profile_with_explicit_region_flag_still_rejected() {
    let home = temp_home();
    write_profile(&home, "prof_a", "key-a", "us1");
    write_profile(&home, "prof_b", "key-b", "us1");
    write_config(&home, "prof_a");

    let output = cx(&home)
        .args([
            "--profile",
            "prof_a",
            "--profile",
            "prof_b",
            "--region",
            "eu1",
            "alerts",
            "list",
        ])
        .output()
        .expect("failed to run cx");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Cannot combine"),
        "explicit --region with multiple profiles should be rejected, stderr: {stderr}"
    );
}
