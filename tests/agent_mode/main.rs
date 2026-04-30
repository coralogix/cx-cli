use assert_cmd::Command;

fn cx() -> Command {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env_remove("CLAUDECODE");
    cmd.env_remove("CLAUDE_CODE");
    cmd.env_remove("CX_AGENT_MODE");
    cmd
}

#[test]
fn agent_mode_blocks_without_yes() {
    let output = cx()
        .env("CX_AGENT_MODE", "1")
        .args([
            "iam",
            "api-keys",
            "delete",
            "nonexistent",
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
        stderr.contains("requires user confirmation"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("--yes"), "stderr: {stderr}");
}

#[test]
fn agent_mode_proceeds_with_yes() {
    let output = cx()
        .env("CX_AGENT_MODE", "1")
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("requires user confirmation"),
        "should not block with --yes, stderr: {stderr}"
    );
    assert!(
        stderr.contains("[auto-approved via --yes]"),
        "stderr: {stderr}"
    );
}

#[test]
fn agent_mode_read_only_takes_precedence() {
    let output = cx()
        .env("CX_AGENT_MODE", "1")
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
        "read-only should take precedence, stderr: {stderr}"
    );
}

#[test]
fn agent_mode_read_commands_unaffected() {
    let output = cx()
        .env("CX_AGENT_MODE", "1")
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
        !stderr.contains("requires user confirmation"),
        "read commands should not be blocked, stderr: {stderr}"
    );
    assert!(!stderr.contains("agent mode"), "stderr: {stderr}");
}

#[test]
fn yes_flag_logs_auto_approve() {
    let output = cx()
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[auto-approved via --yes]"),
        "stderr: {stderr}"
    );
}

#[test]
fn read_command_no_auto_approve_output() {
    let output = cx()
        .args([
            "iam",
            "api-keys",
            "list",
            "--api-key",
            "fake",
            "--region",
            "us1",
            "--yes",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("[auto-approved"),
        "read commands should not show auto-approve, stderr: {stderr}"
    );
}
