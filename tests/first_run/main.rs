//! Integration tests for first-run guidance (FORGE-659).
//!
//! When any credential-requiring command runs with no profile configured, the
//! CLI should exit non-zero with a single actionable next step pointing to
//! `cx init` — not a stack of config-resolution errors. The onboarding commands
//! that *fix* the missing-profile state (`cx init`, `cx profiles add`,
//! `cx skills`) are handled before profile resolution and must never be
//! short-circuited by this guidance.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cx_test_first_run_{tag}_{pid}_{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A `cx` command hermetically sealed off from the developer's real home and
/// environment credentials — the fresh-machine, no-profile state.
fn cx(home: &Path) -> Command {
    let cwd = home.join("project");
    fs::create_dir_all(&cwd).unwrap();
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("HOME", home)
        .env("CX_HOME", home)
        // Never inherit the developer's credentials into the sandbox, so the
        // resolver sees a genuinely unconfigured machine.
        .env_remove("CX_API_KEY")
        .env_remove("CX_REGION")
        .env_remove("CX_PROFILE")
        .current_dir(cwd);
    cmd
}

// ── First-run guidance fires for credential-requiring commands ────────────────

#[test]
fn no_profile_points_to_cx_init() {
    let home = temp_dir("nag");

    let assert = cx(&home)
        .args(["logs", "source logs | limit 1"])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("No Coralogix profile is configured"),
        "stderr should state the machine is unconfigured: {stderr}"
    );
    assert!(
        stderr.contains("cx init"),
        "stderr should point at `cx init`: {stderr}"
    );
    // The guidance must be the *entire* output: no anyhow chain dumped after
    // it, and no second, contradicting instruction. Assert on the whole
    // stderr, not just the absence of the generic wrapper string.
    assert!(
        !stderr.to_lowercase().contains("error:") && !stderr.contains("Caused by:"),
        "first-run guidance must not be followed by the raw error chain: {stderr}"
    );
    assert!(
        !stderr.contains("cx profiles add"),
        "first-run guidance must give exactly one next step (`cx init`): {stderr}"
    );
}

// ── Onboarding commands are never short-circuited ─────────────────────────────

#[test]
fn profiles_list_is_not_short_circuited() {
    let home = temp_dir("profiles_list");

    let assert = cx(&home).args(["profiles", "list"]).assert().success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    // `cx profiles list` is handled before resolution, so it reports its own
    // empty state rather than the first-run `cx init` nag.
    assert!(
        stdout.contains("No profiles configured"),
        "profiles list should print its own empty-state message: {stdout}"
    );
    assert!(
        !stdout.contains("cx init"),
        "profiles list must not be redirected to `cx init`: {stdout}"
    );
}
