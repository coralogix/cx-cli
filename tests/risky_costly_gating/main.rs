use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_home() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cx_test_{pid}_{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn set_home(cmd: &mut Command, home: &std::path::Path) {
    cmd.env("HOME", home);
    // On Windows, dirs::home_dir() reads USERPROFILE instead of HOME.
    #[cfg(windows)]
    cmd.env("USERPROFILE", home);
}

fn cx_with_config(home: &std::path::Path, config_toml: &str) -> Command {
    let cx_dir = home.join(".cx");
    fs::create_dir_all(&cx_dir).unwrap();
    fs::write(cx_dir.join("config.toml"), config_toml).unwrap();

    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    set_home(&mut cmd, home);
    cmd
}

// ── Risky command gating ─────────────────────────────────────────────────────

#[test]
fn risky_disabled_blocks_iam_write() {
    let tmp = temp_home();
    let output = cx_with_config(&tmp, "allow_risky_commands = false\n")
        .args([
            "iam",
            "api-keys",
            "delete",
            "abc",
            "--api-key",
            "fake",
            "--region",
            "us1",
        ])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Risky write operation blocked"),
        "stderr: {stderr}"
    );
}

#[test]
fn risky_disabled_allows_iam_read() {
    let tmp = temp_home();
    let output = cx_with_config(&tmp, "allow_risky_commands = false\n")
        .args([
            "iam",
            "api-keys",
            "list",
            "--api-key",
            "fake",
            "--region",
            "us1",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Risky write operation blocked"),
        "read command should not be blocked, stderr: {stderr}"
    );
}

#[test]
fn risky_disabled_blocks_archive_write() {
    let tmp = temp_home();
    let output = cx_with_config(&tmp, "allow_risky_commands = false\n")
        .args([
            "archive",
            "metrics",
            "enable",
            "--api-key",
            "fake",
            "--region",
            "us1",
        ])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Risky write operation blocked"),
        "stderr: {stderr}"
    );
}

#[test]
fn risky_enabled_allows_iam_write() {
    let tmp = temp_home();
    let output = cx_with_config(&tmp, "allow_risky_commands = true\n")
        .args([
            "iam",
            "api-keys",
            "delete",
            "abc",
            "--api-key",
            "fake",
            "--region",
            "us1",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Risky write operation blocked"),
        "should not be blocked when enabled, stderr: {stderr}"
    );
}

// ── Costly command gating ────────────────────────────────────────────────────

#[test]
fn costly_disabled_blocks_olly_ask() {
    let tmp = temp_home();
    let output = cx_with_config(&tmp, "allow_costly_commands = false\n")
        .args([
            "olly",
            "ask",
            "test question",
            "--api-key",
            "fake",
            "--region",
            "us1",
        ])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("additional charges"), "stderr: {stderr}");
}

#[test]
fn costly_disabled_allows_olly_artifacts() {
    let tmp = temp_home();
    let output = cx_with_config(&tmp, "allow_costly_commands = false\n")
        .args([
            "olly",
            "artifacts",
            "list",
            "--api-key",
            "fake",
            "--region",
            "us1",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("additional charges"),
        "artifacts should not be blocked, stderr: {stderr}"
    );
}

#[test]
fn costly_enabled_allows_olly_ask() {
    let tmp = temp_home();
    let output = cx_with_config(&tmp, "allow_costly_commands = true\n")
        .args([
            "olly",
            "ask",
            "test question",
            "--api-key",
            "fake",
            "--region",
            "us1",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("additional charges"),
        "should not be blocked when enabled, stderr: {stderr}"
    );
}

// ── Read-only config gating ──────────────────────────────────────────────────

#[test]
fn read_only_config_blocks_write() {
    let tmp = temp_home();
    let output = cx_with_config(&tmp, "read_only = true\n")
        .args([
            "alerts",
            "create",
            "--from-file",
            "x.json",
            "--api-key",
            "fake",
            "--region",
            "us1",
        ])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("read-only mode"), "stderr: {stderr}");
}

#[test]
fn read_only_config_allows_read() {
    let tmp = temp_home();
    let output = cx_with_config(&tmp, "read_only = true\n")
        .args(["alerts", "list", "--api-key", "fake", "--region", "us1"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("read-only mode"),
        "read command should not be blocked, stderr: {stderr}"
    );
}

// ── Default behavior (no config) ─────────────────────────────────────────────

#[test]
fn no_config_file_allows_everything() {
    let tmp = temp_home();
    // No config file at all - should default to permissive.
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    set_home(&mut cmd, &tmp);
    let output = cmd
        .args([
            "iam",
            "api-keys",
            "delete",
            "abc",
            "--api-key",
            "fake",
            "--region",
            "us1",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Risky write operation blocked"),
        "should not be blocked without config, stderr: {stderr}"
    );
    assert!(
        !stderr.contains("read-only mode"),
        "should not be read-only without config, stderr: {stderr}"
    );
}
