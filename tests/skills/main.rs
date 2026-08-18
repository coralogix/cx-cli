//! Integration tests for `cx skills install` (FORGE-657).
//!
//! The installer is exercised through a fake `npx` script placed on PATH that
//! records the arguments it receives, so no Node.js or network is required.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cx_test_skills_{tag}_{pid}_{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A `cx` command hermetically sealed off from the developer's real home,
/// project directory, and PATH.
fn cx(home: &Path, path_dir: &Path) -> Command {
    let cwd = home.join("project");
    fs::create_dir_all(&cwd).unwrap();
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("HOME", home)
        .env("CX_HOME", home)
        .env("PATH", path_dir)
        .current_dir(cwd);
    cmd
}

/// Install a fake `npx` into `dir` that answers `--version`, serves `ls_json`
/// for `skills ls` (the already-installed detection), and records any other
/// invocation's arguments into `args_file`.
#[cfg(unix)]
fn install_fake_npx(dir: &Path, args_file: &Path, ls_json: &str) {
    use std::os::unix::fs::PermissionsExt;
    // Only `echo` (a shell builtin) is usable here: PATH contains nothing but
    // this directory, so external tools like `cat` would not resolve.
    assert!(
        !ls_json.contains('\''),
        "ls_json must not contain single quotes"
    );
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo \"10.0.0\"; exit 0; fi\n\
         if [ \"$2\" = \"skills\" ] && [ \"$3\" = \"ls\" ]; then echo '{ls_json}'; exit 0; fi\n\
         echo \"$@\" > \"{}\"\n",
        args_file.display()
    );
    let npx = dir.join("npx");
    fs::write(&npx, script).unwrap();
    fs::set_permissions(&npx, fs::Permissions::from_mode(0o755)).unwrap();
}

// ── Missing npx ───────────────────────────────────────────────────────────────

#[test]
fn install_without_npx_fails_with_actionable_error() {
    let home = temp_dir("no_npx");
    let empty_path = temp_dir("no_npx_path");
    let output = cx(&home, &empty_path)
        .args(["skills", "install", "--global"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Node.js") && stderr.contains("npx"),
        "expected actionable Node.js/npx error, stderr: {stderr}"
    );
}

// ── Non-interactive scope resolution ──────────────────────────────────────────

#[cfg(unix)]
#[test]
fn install_without_scope_and_no_tty_fails_naming_the_flags() {
    let home = temp_dir("no_scope");
    let bin = temp_dir("no_scope_bin");
    install_fake_npx(&bin, &bin.join("args.txt"), "[]");
    let output = cx(&home, &bin)
        .args(["skills", "install"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--global") && stderr.contains("--local"),
        "expected error naming --global/--local, stderr: {stderr}"
    );
}

// ── Non-interactive install shell-out ─────────────────────────────────────────

#[cfg(unix)]
#[test]
fn install_global_runs_fully_noninteractive_command() {
    let home = temp_dir("noninteractive_global");
    let bin = temp_dir("noninteractive_global_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file, "[]");

    cx(&home, &bin)
        .args(["skills", "install", "--global"])
        .assert()
        .success();

    let recorded = fs::read_to_string(&args_file).unwrap();
    assert_eq!(
        recorded.trim(),
        "-y skills add coralogix/cx-cli --skill * -y -g"
    );
}

#[cfg(unix)]
#[test]
fn install_local_with_agents_passes_them_through() {
    let home = temp_dir("noninteractive_local");
    let bin = temp_dir("noninteractive_local_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file, "[]");

    cx(&home, &bin)
        .args([
            "skills",
            "install",
            "--local",
            "--agent",
            "claude-code",
            "--agent",
            "cursor",
        ])
        .assert()
        .success();

    let recorded = fs::read_to_string(&args_file).unwrap();
    assert_eq!(
        recorded.trim(),
        "-y skills add coralogix/cx-cli --skill * -y -a claude-code cursor"
    );
}

// ── Advanced (interactive) walk ───────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn install_interactive_runs_the_raw_installer() {
    let home = temp_dir("advanced");
    let bin = temp_dir("advanced_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file, "[]");

    cx(&home, &bin)
        .args(["skills", "install", "--interactive"])
        .assert()
        .success();

    let recorded = fs::read_to_string(&args_file).unwrap();
    assert_eq!(recorded.trim(), "skills add coralogix/cx-cli");
}

// ── Already-installed detection (explicit run reinstalls) ─────────────────────

/// Detection asks the installer itself (`skills ls --json`); the fake npx
/// reports one cx skill already installed.
#[cfg(unix)]
#[test]
fn install_over_existing_skills_notes_the_update() {
    let home = temp_dir("preinstalled");
    let bin = temp_dir("preinstalled_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(
        &bin,
        &args_file,
        r#"[{"name": "cx-alerts", "path": "/skills/cx-alerts", "scope": "global",
             "agents": ["Claude Code"], "source": "coralogix/cx-cli",
             "sourceUrl": null, "sourceType": "github"}]"#,
    );

    let output = cx(&home, &bin)
        .args(["skills", "install", "--global"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("already installed") && stdout.contains("cx-alerts"),
        "expected already-installed note naming the skill, stdout: {stdout}"
    );
    // Explicit install still runs the installer (reinstall/update).
    assert!(args_file.exists(), "installer should still have been run");
}

// ── Flag conflicts ────────────────────────────────────────────────────────────

#[test]
fn global_and_local_conflict() {
    let home = temp_dir("conflict");
    let bin = temp_dir("conflict_bin");
    cx(&home, &bin)
        .args(["skills", "install", "--global", "--local"])
        .assert()
        .failure();
}
