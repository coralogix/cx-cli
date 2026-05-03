use assert_cmd::Command;

fn cx() -> Command {
    Command::cargo_bin("cx").expect("cx binary should build")
}

#[test]
fn read_only_flag_blocks_write_command() {
    let output = cx()
        .args([
            "--read-only",
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
    assert!(stderr.contains("read-only mode"), "stderr: {stderr}");
}

#[test]
fn read_only_env_var_blocks_write_command() {
    let output = cx()
        .env("CX_READ_ONLY", "1")
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
    assert!(stderr.contains("read-only mode"), "stderr: {stderr}");
}

#[test]
fn read_only_flag_allows_read_command() {
    let output = cx()
        .args([
            "--read-only",
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
        !stderr.contains("read-only mode"),
        "read command should not be blocked, stderr: {stderr}"
    );
}

#[test]
fn read_only_env_true_blocks_write() {
    let output = cx()
        .env("CX_READ_ONLY", "true")
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
    assert!(stderr.contains("read-only mode"), "stderr: {stderr}");
}

#[test]
fn read_only_env_yes_blocks_write() {
    let output = cx()
        .env("CX_READ_ONLY", "yes")
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
    assert!(stderr.contains("read-only mode"), "stderr: {stderr}");
}
