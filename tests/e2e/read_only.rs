use crate::harness;

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
    let output = harness::cx()
        .args(["--read-only", "iam", "api-keys", "delete", "nonexistent", "--yes", "--api-key", "fake", "--region", "us1"])
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
    let output = harness::cx()
        .env("CX_READ_ONLY", "1")
        .args(["iam", "api-keys", "delete", "nonexistent", "--yes", "--api-key", "fake", "--region", "us1"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("read-only mode"),
        "expected read-only error, stderr: {stderr}"
    );
}
