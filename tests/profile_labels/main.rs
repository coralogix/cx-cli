//! The global `--label` selector: run a command against every profile that
//! has one of the given labels, fanning out like repeated `--profile`.

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
    let dir = std::env::temp_dir().join(format!("cx_profile_labels_test_{pid}_{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_profile(home: &std::path::Path, name: &str, api_key: &str, region: &str, labels: &str) {
    let profiles_dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    let content = format!(
        r#"auth = "api_key"
credential_storage = "file"
api_key = "{api_key}"
region = "{region}"
{labels}
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

/// Mount an alerts-list mock that only answers for `key`, and expect exactly
/// `hits` calls.
async fn alerts_mock(server: &MockServer, key: &str, hits: u64) {
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .and(header("Authorization", &*format!("Bearer {key}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "alertDefs": []
        })))
        .expect(hits)
        .mount(server)
        .await;
}

// ── --label selects every matching profile ───────────────────────────────────

#[tokio::test]
async fn label_fans_out_to_every_profile_with_that_label() {
    let prod_a = MockServer::start().await;
    let prod_b = MockServer::start().await;
    let staging = MockServer::start().await;
    alerts_mock(&prod_a, "key-a", 1).await;
    alerts_mock(&prod_b, "key-b", 1).await;
    alerts_mock(&staging, "key-s", 0).await;

    let home = temp_home();
    write_profile(
        &home,
        "prod-a",
        "key-a",
        &prod_a.uri(),
        r#"labels = ["prod", "eu"]"#,
    );
    write_profile(
        &home,
        "prod-b",
        "key-b",
        &prod_b.uri(),
        r#"labels = ["prod", "us"]"#,
    );
    write_profile(
        &home,
        "staging",
        "key-s",
        &staging.uri(),
        r#"labels = ["staging"]"#,
    );
    write_config(&home, "staging");

    let output = cx(&home)
        .args(["alerts", "list", "--label", "prod", "-o", "json"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(
        !stderr.contains("Error") && !stderr.contains("error"),
        "no profile should fail, stderr: {stderr}"
    );
}

#[tokio::test]
async fn label_accepts_repeated_and_comma_separated_values() {
    let eu = MockServer::start().await;
    let us = MockServer::start().await;
    let ap = MockServer::start().await;
    alerts_mock(&eu, "key-eu", 1).await;
    alerts_mock(&us, "key-us", 1).await;
    alerts_mock(&ap, "key-ap", 1).await;

    let home = temp_home();
    write_profile(
        &home,
        "eu",
        "key-eu",
        &eu.uri(),
        r#"labels = ["region:eu1"]"#,
    );
    write_profile(
        &home,
        "us",
        "key-us",
        &us.uri(),
        r#"labels = ["region:us2"]"#,
    );
    write_profile(
        &home,
        "ap",
        "key-ap",
        &ap.uri(),
        r#"labels = ["region:ap1"]"#,
    );
    write_config(&home, "eu");

    cx(&home)
        .args([
            "alerts",
            "list",
            "--label",
            "region:eu1,region:us2",
            "--label",
            "region:ap1",
        ])
        .assert()
        .success();
}

#[tokio::test]
async fn label_before_the_command_works_too() {
    let a = MockServer::start().await;
    let b = MockServer::start().await;
    alerts_mock(&a, "key-a", 1).await;
    alerts_mock(&b, "key-b", 1).await;

    let home = temp_home();
    write_profile(
        &home,
        "a",
        "key-a",
        &a.uri(),
        r#"labels = ["customer:acme"]"#,
    );
    write_profile(
        &home,
        "b",
        "key-b",
        &b.uri(),
        r#"labels = ["customer:acme"]"#,
    );
    write_config(&home, "a");

    cx(&home)
        .args(["--label", "customer:acme", "alerts", "list"])
        .assert()
        .success();
}

#[tokio::test]
async fn label_matches_legacy_single_label_key() {
    let server = MockServer::start().await;
    alerts_mock(&server, "key-old", 1).await;

    let home = temp_home();
    write_profile(&home, "old", "key-old", &server.uri(), r#"label = "prod""#);
    write_config(&home, "old");

    cx(&home)
        .args(["alerts", "list", "--label", "prod"])
        .assert()
        .success();
}

// ── --label combines with --profile ──────────────────────────────────────────

#[tokio::test]
async fn label_and_profile_combine_without_duplicates() {
    let named = MockServer::start().await;
    let tagged = MockServer::start().await;
    // `named` is selected by both flags; it must still run exactly once.
    alerts_mock(&named, "key-n", 1).await;
    alerts_mock(&tagged, "key-t", 1).await;

    let home = temp_home();
    write_profile(
        &home,
        "named",
        "key-n",
        &named.uri(),
        r#"labels = ["gold"]"#,
    );
    write_profile(
        &home,
        "tagged",
        "key-t",
        &tagged.uri(),
        r#"labels = ["gold"]"#,
    );
    write_config(&home, "named");

    cx(&home)
        .args(["alerts", "list", "-p", "named", "--label", "gold"])
        .assert()
        .success();
}

// ── Errors ───────────────────────────────────────────────────────────────────

#[test]
fn label_with_no_match_is_an_error_not_a_silent_default() {
    let home = temp_home();
    write_profile(&home, "only", "key", "eu1", r#"labels = ["prod"]"#);
    write_config(&home, "only");

    let output = cx(&home)
        .args(["alerts", "list", "--label", "staging"])
        .output()
        .expect("failed to run cx");

    assert!(!output.status.success(), "a label with no match must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No profiles with label 'staging'."),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("cx profiles list"), "stderr: {stderr}");
}

#[test]
fn label_matching_several_profiles_rejects_api_key_override() {
    let home = temp_home();
    write_profile(&home, "a", "key-a", "eu1", r#"labels = ["prod"]"#);
    write_profile(&home, "b", "key-b", "eu1", r#"labels = ["prod"]"#);
    write_config(&home, "a");

    let output = cx(&home)
        .args([
            "alerts",
            "list",
            "--label",
            "prod",
            "--api-key",
            "explicit-key",
        ])
        .output()
        .expect("failed to run cx");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Cannot combine"),
        "--api-key with a multi-profile label should be rejected, stderr: {stderr}"
    );
}

#[tokio::test]
async fn label_on_the_command_line_ignores_env_api_key() {
    let server = MockServer::start().await;
    // The profile's own key must be used, not the env var.
    alerts_mock(&server, "profile-key", 1).await;

    let home = temp_home();
    write_profile(
        &home,
        "one",
        "profile-key",
        &server.uri(),
        r#"labels = ["prod"]"#,
    );
    write_config(&home, "one");

    cx(&home)
        .env("CX_API_KEY", "env-key-should-be-ignored")
        .args(["alerts", "list", "--label", "prod"])
        .assert()
        .success();
}

// ── `profiles list` keeps its own --label, in both parse paths ───────────────

#[test]
fn profiles_list_label_before_profiles_still_filters() {
    let home = temp_home();
    write_profile(&home, "a", "k", "eu1", r#"labels = ["prod"]"#);
    write_profile(&home, "b", "k", "eu1", r#"labels = ["staging"]"#);
    write_config(&home, "a");

    let output = cx(&home)
        .args(["--label", "prod", "profiles", "list", "-o", "json"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"a\""), "stdout: {stdout}");
    assert!(!stdout.contains("\"b\""), "stdout: {stdout}");
}
