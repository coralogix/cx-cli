use assert_cmd::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Return a fresh temporary directory to use as HOME, so that tests are not
/// affected by the user's real `~/.cx/config.toml` (which may set `read_only`).
fn temp_home() -> std::path::PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("cx_agent_test_{}_{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cx() -> Command {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("CX_HOME", temp_home());
    cmd.env_remove("CX_SKILL_AGENT_NAME");
    cmd
}

#[test]
fn agent_mode_blocks_without_yes() {
    let output = cx()
        .env("CX_SKILL_AGENT_NAME", "test-agent")
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
        .env("CX_SKILL_AGENT_NAME", "test-agent")
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
        .env("CX_SKILL_AGENT_NAME", "test-agent")
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
        .env("CX_SKILL_AGENT_NAME", "test-agent")
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
