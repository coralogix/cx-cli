use assert_cmd::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_home() -> std::path::PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("cx_ro_test_{}_{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cx() -> Command {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("CX_HOME", temp_home());
    cmd
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

fn context_under_read_only(args: &[&str]) -> String {
    let output = cx()
        .env_remove("CX_READ_ONLY")
        .args([
            "--read-only",
            "--api-key",
            "fake",
            "--region",
            "http://127.0.0.1:9",
        ])
        .args(["context"])
        .args(args)
        .output()
        .expect("failed to run cx");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn read_only_blocks_context_writes() {
    let writes: [&[&str]; 5] = [
        &[
            "memories",
            "revert",
            "x",
            "--to-revision",
            "1",
            "--expected-revision",
            "2",
        ],
        &[
            "instructions",
            "revert",
            "x",
            "--to-revision",
            "1",
            "--expected-revision",
            "2",
        ],
        &[
            "memories",
            "update",
            "x",
            "--expected-revision",
            "1",
            "--title",
            "T",
        ],
        &["instructions", "delete", "x", "--expected-revision", "1"],
        &[
            "memories",
            "create",
            "--title",
            "T",
            "--summary",
            "S",
            "--team-wide",
            "--body-file",
            "-",
        ],
    ];
    for args in writes {
        let stderr = context_under_read_only(args);
        assert!(stderr.contains("read-only mode"), "{args:?}: {stderr}");
    }
}

#[test]
fn read_only_allows_context_reads() {
    let reads: [&[&str]; 5] = [
        &["get"],
        &["history"],
        &["memories", "list"],
        &["instructions", "get", "x"],
        &["memories", "history", "x"],
    ];
    for args in reads {
        let stderr = context_under_read_only(args);
        assert!(!stderr.contains("read-only mode"), "{args:?}: {stderr}");
        assert!(
            stderr.contains("error sending request"),
            "{args:?}: {stderr}"
        );
    }
}
