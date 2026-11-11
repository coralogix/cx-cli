//! Read-only mode: `--read-only`, `CX_READ_ONLY`, and `read_only = true` in
//! config.toml all block write verbs, while reads and local commands still
//! run. The gating tests need no network, so they are not `#[ignore]`d.
//! `tests/read_only` covers the flag/env truthiness matrix; this file covers
//! the config-file source and the local-command allowlist.

use std::fs;

use crate::harness;
use crate::profiles_and_local::{cx_in_home, temp_home};

const FAKE_CREDS: [&str; 4] = ["--api-key", "fake", "--region", "us1"];

#[test]
#[ignore]
fn read_only_allows_read_command() {
    if harness::require_creds("read_only_allows_read_command").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["--read-only", "iam", "api-keys", "list", "-o", "json"]);
    // Query commands are reads too.
    let _v = crate::logs::query_json(&[
        "--read-only",
        "logs",
        "limit 1",
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "1",
        "-o",
        "json",
    ]);
}

#[test]
fn read_only_blocks_write_command() {
    let output = harness::cx()
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
    let output = harness::cx()
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

/// `read_only = true` in config.toml is the third source of read-only mode.
#[test]
fn read_only_config_blocks_write_command() {
    let home = temp_home("ro_config");
    fs::create_dir_all(home.join(".cx")).unwrap();
    fs::write(home.join(".cx").join("config.toml"), "read_only = true\n").unwrap();
    for args in [
        vec!["alerts", "delete", "nonexistent", "--yes"],
        vec!["dashboards", "folders", "create", "--name", "x", "--yes"],
        vec![
            "views",
            "update",
            "nonexistent",
            "--from-file",
            "/dev/null",
            "--yes",
        ],
    ] {
        let output = cx_in_home(&home)
            .args(&args)
            .args(FAKE_CREDS)
            .output()
            .expect("failed to run cx");
        assert!(!output.status.success(), "{args:?} must be blocked");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("read-only mode"), "{args:?}: {stderr}");
    }
}

/// Local commands (profiles, cleanup, completions, dataprime docs, schema)
/// are never writes in the read-only sense and keep working.
#[test]
fn read_only_allows_local_commands() {
    let home = temp_home("ro_local");
    for args in [
        vec!["--read-only", "profiles", "list"],
        vec!["--read-only", "cleanup"],
        vec!["--read-only", "completions", "generate", "bash"],
        vec![
            "--read-only",
            "dataprime",
            "list",
            "--filter",
            "commands",
            "-o",
            "json",
        ],
        vec!["--read-only", "dataprime", "show", "filter"],
        vec!["--read-only", "schema"],
    ] {
        let output = cx_in_home(&home)
            .env("CX_READ_ONLY", "1")
            .args(&args)
            .output()
            .expect("failed to run cx");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "{args:?} must run in read-only mode: {stderr}"
        );
        assert!(!stderr.contains("read-only mode"), "{args:?}: {stderr}");
    }
}
