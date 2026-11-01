use crate::harness;
use std::path::PathBuf;

fn cx_without_profiles() -> assert_cmd::Command {
    let home = std::env::temp_dir().join(format!("cx_e2e_read_only_{}", std::process::id()));
    let mut command = harness::cx();
    command.env("CX_HOME", &home).env(
        "CX_TEST_KEYRING_DIR",
        PathBuf::from(&home).join(".test-keyring"),
    );
    command
}

#[test]
#[ignore]
fn read_only_allows_read_command() {
    if harness::require_creds("read_only_allows_read_command").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["--read-only", "iam", "api-keys", "list", "-o", "json"]);
}

#[test]
fn read_only_blocks_write_command() {
    let output = cx_without_profiles()
        .args([
            "--read-only",
            "iam",
            "api-keys",
            "delete",
            "nonexistent",
            "--yes",
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
        stderr.contains("read-only mode"),
        "expected read-only error, stderr: {stderr}"
    );
}

#[test]
fn read_only_env_blocks_write_command() {
    let output = cx_without_profiles()
        .env("CX_READ_ONLY", "1")
        .args([
            "iam",
            "api-keys",
            "delete",
            "nonexistent",
            "--yes",
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
        stderr.contains("read-only mode"),
        "expected read-only error, stderr: {stderr}"
    );
}
