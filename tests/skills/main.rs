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

/// Install a fake `npx` into `dir` that answers `--version`, serves
/// `project_json` for `skills ls --json` and `global_json` for `skills ls -g
/// --json` (the scope-aware already-installed detection), and records any
/// other invocation's arguments into `args_file`.
#[cfg(unix)]
fn install_fake_npx(dir: &Path, args_file: &Path, project_json: &str, global_json: &str) {
    use std::os::unix::fs::PermissionsExt;
    // Only `echo` (a shell builtin) is usable here: PATH contains nothing but
    // this directory, so external tools like `cat` would not resolve.
    for json in [project_json, global_json] {
        assert!(
            !json.contains('\''),
            "ls json must not contain single quotes"
        );
    }
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo \"10.0.0\"; exit 0; fi\n\
         if [ \"$2\" = \"skills\" ] && [ \"$3\" = \"ls\" ]; then\n\
           for a in \"$@\"; do\n\
             if [ \"$a\" = \"-g\" ]; then echo '{global_json}'; exit 0; fi\n\
           done\n\
           echo '{project_json}'; exit 0\n\
         fi\n\
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
    install_fake_npx(&bin, &bin.join("args.txt"), "[]", "[]");
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
    install_fake_npx(&bin, &args_file, "[]", "[]");

    cx(&home, &bin)
        .args(["skills", "install", "--global"])
        .assert()
        .success();

    let recorded = fs::read_to_string(&args_file).unwrap();
    assert_eq!(
        recorded.trim(),
        format!(
            "-y skills add coralogix/cx-cli/skills#v{} --skill * -y -g",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[cfg(unix)]
#[test]
fn install_local_with_agents_passes_them_through() {
    let home = temp_dir("noninteractive_local");
    let bin = temp_dir("noninteractive_local_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file, "[]", "[]");

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
        format!(
            "-y skills add coralogix/cx-cli/skills#v{} --skill * -y -a claude-code cursor",
            env!("CARGO_PKG_VERSION")
        )
    );
}

// ── Advanced (interactive) walk ───────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn install_interactive_runs_the_raw_installer() {
    let home = temp_dir("advanced");
    let bin = temp_dir("advanced_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file, "[]", "[]");

    cx(&home, &bin)
        .args(["skills", "install", "--interactive"])
        .assert()
        .success();

    let recorded = fs::read_to_string(&args_file).unwrap();
    assert_eq!(
        recorded.trim(),
        format!(
            "skills add coralogix/cx-cli/skills#v{}",
            env!("CARGO_PKG_VERSION")
        )
    );
}

// ── Already-installed detection (explicit run reinstalls) ─────────────────────

/// Detection asks the installer itself (`skills ls -g --json`); the fake npx
/// reports one cx skill already installed globally.
#[cfg(unix)]
#[test]
fn install_over_existing_skills_notes_the_update() {
    let home = temp_dir("preinstalled");
    let bin = temp_dir("preinstalled_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(
        &bin,
        &args_file,
        "[]",
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

/// Detection is scoped: skills present only globally must not be reported as
/// already-installed for a `--local` install.
#[cfg(unix)]
#[test]
fn install_local_ignores_globally_installed_skills() {
    let home = temp_dir("scoped");
    let bin = temp_dir("scoped_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(
        &bin,
        &args_file,
        "[]",
        r#"[{"name": "cx-alerts", "source": "coralogix/cx-cli"}]"#,
    );

    let output = cx(&home, &bin)
        .args(["skills", "install", "--local"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("already installed"),
        "global skills must not count for a local install, stdout: {stdout}"
    );
    assert!(args_file.exists(), "installer should still have been run");
}

// ── Verbose installer output is suppressed for the -y install ─────────────────

/// The non-interactive install captures the installer's verbose TUI and prints
/// a compact summary instead — the banner must not reach the user, but the
/// count and the "review before use" pointer must.
#[cfg(unix)]
#[test]
fn noninteractive_install_hides_installer_output_and_summarizes() {
    use std::os::unix::fs::PermissionsExt;
    let home = temp_dir("summary");
    let bin = temp_dir("summary_bin");

    // A fake npx that spews a banner to stdout on `skills add`, and reports two
    // cx skills installed for `skills ls --json`.
    let ls_json = r#"[{"name":"cx-alerts","source":"coralogix/cx-cli"},
                      {"name":"cx-dashboards","source":"coralogix/cx-cli"}]"#;
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo \"10.0.0\"; exit 0; fi\n\
         if [ \"$2\" = \"skills\" ] && [ \"$3\" = \"ls\" ]; then echo '{ls_json}'; exit 0; fi\n\
         echo 'BANNER_LINE_SHOULD_BE_HIDDEN'; exit 0\n"
    );
    let npx = bin.join("npx");
    fs::write(&npx, script).unwrap();
    fs::set_permissions(&npx, fs::Permissions::from_mode(0o755)).unwrap();

    let output = cx(&home, &bin)
        .args(["skills", "install", "--global"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("BANNER_LINE_SHOULD_BE_HIDDEN"),
        "installer's verbose output must be suppressed, stdout: {stdout}"
    );
    assert!(
        stdout.contains("Installed 2 cx agent skills") && stdout.contains("Browse them at"),
        "expected the compact summary with a skill-listing pointer, stdout: {stdout}"
    );
}

/// On failure, the installer's diagnostics are surfaced (not swallowed).
#[cfg(unix)]
#[test]
fn noninteractive_install_failure_surfaces_installer_stderr() {
    use std::os::unix::fs::PermissionsExt;
    let home = temp_dir("failout");
    let bin = temp_dir("failout_bin");

    let script = "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo \"10.0.0\"; exit 0; fi\n\
         if [ \"$2\" = \"skills\" ] && [ \"$3\" = \"ls\" ]; then echo '[]'; exit 0; fi\n\
         echo 'INSTALLER_BOOM' 1>&2; exit 1\n";
    let npx = bin.join("npx");
    fs::write(&npx, script).unwrap();
    fs::set_permissions(&npx, fs::Permissions::from_mode(0o755)).unwrap();

    let output = cx(&home, &bin)
        .args(["skills", "install", "--global"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("INSTALLER_BOOM"),
        "installer diagnostics must be surfaced on failure, stderr: {stderr}"
    );
}

/// Node CLIs often report errors on stdout; a failing install must surface
/// those diagnostics too, not just stderr.
#[cfg(unix)]
#[test]
fn noninteractive_install_failure_surfaces_installer_stdout() {
    use std::os::unix::fs::PermissionsExt;
    let home = temp_dir("failout_stdout");
    let bin = temp_dir("failout_stdout_bin");

    let script = "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo \"10.0.0\"; exit 0; fi\n\
         if [ \"$2\" = \"skills\" ] && [ \"$3\" = \"ls\" ]; then echo '[]'; exit 0; fi\n\
         echo 'INSTALLER_STDOUT_BOOM'; exit 1\n";
    let npx = bin.join("npx");
    fs::write(&npx, script).unwrap();
    fs::set_permissions(&npx, fs::Permissions::from_mode(0o755)).unwrap();

    let output = cx(&home, &bin)
        .args(["skills", "install", "--global"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("INSTALLER_STDOUT_BOOM"),
        "installer stdout diagnostics must be surfaced on failure, stderr: {stderr}"
    );
}

/// Fail-open contract (see `installed_cx_skills`): an old installer without
/// `--json` prints human-formatted text on exit 0, which fails to parse as
/// JSON. That must count as nothing installed and let the install proceed,
/// rather than erroring out.
#[cfg(unix)]
#[test]
fn install_treats_unparseable_ls_output_as_nothing_installed() {
    let home = temp_dir("unparseable_ls");
    let bin = temp_dir("unparseable_ls_bin");
    let args_file = bin.join("args.txt");
    // Realistic old-installer output: a human-formatted table, not JSON.
    let plain = "cx-alerts  global  Claude Code  coralogix/cx-cli";
    install_fake_npx(&bin, &args_file, plain, plain);

    let output = cx(&home, &bin)
        .args(["skills", "install", "--global"])
        .output()
        .expect("failed to run cx");
    assert!(
        output.status.success(),
        "unparseable ls output must fail open, not error the install"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("already installed"),
        "unparseable ls output must count as nothing installed, stdout: {stdout}"
    );
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

// ── Recorded skills version (HEA-62) ──────────────────────────────────────────

/// The `skills_installed_for` value cx saved in `state.json`, if any.
fn recorded_skills_version(home: &Path) -> Option<String> {
    let contents = fs::read_to_string(home.join(".cx").join("state.json")).ok()?;
    let state: serde_json::Value = serde_json::from_str(&contents).ok()?;
    state["skills_installed_for"].as_str().map(str::to_string)
}

#[cfg(unix)]
#[test]
fn global_install_records_the_cx_version() {
    let home = temp_dir("record_global");
    let bin = temp_dir("record_global_bin");
    install_fake_npx(&bin, &bin.join("args.txt"), "[]", "[]");

    cx(&home, &bin)
        .env("CX_NO_UPDATE_NOTIFIER", "1")
        .args(["skills", "install", "--global"])
        .assert()
        .success();

    assert_eq!(
        recorded_skills_version(&home).as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}

/// Local skills belong to one project, and `--interactive` may be cancelled
/// (the installer still exits 0), so neither marks the skills as current.
#[cfg(unix)]
#[test]
fn local_and_interactive_installs_record_nothing() {
    for flag in ["--local", "--interactive"] {
        let home = temp_dir("record_none");
        let bin = temp_dir("record_none_bin");
        install_fake_npx(&bin, &bin.join("args.txt"), "[]", "[]");

        cx(&home, &bin)
            .env("CX_NO_UPDATE_NOTIFIER", "1")
            .args(["skills", "install", flag])
            .assert()
            .success();

        assert_eq!(recorded_skills_version(&home), None, "{flag}");
    }
}

// ── Release-tag pin, recorded agents, background refresh (HEA-62) ─────────────

/// The `~/.cx/state.json` that cx reads with `CX_HOME=home`.
fn state_path(home: &Path) -> PathBuf {
    home.join(".cx").join("state.json")
}

fn read_state(home: &Path) -> serde_json::Value {
    fs::read_to_string(state_path(home))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Poll for up to 10 s: the background refresh runs in a detached process.
fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if done() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

/// Install a fake `npx` whose tagged (`#vX.Y.Z`) install fails with
/// `tag_failure` on stdout, and whose untagged install records its arguments
/// into `args_file`.
#[cfg(unix)]
fn install_fake_npx_failing_tag(dir: &Path, args_file: &Path, tag_failure: &str) {
    use std::os::unix::fs::PermissionsExt;
    assert!(!tag_failure.contains('\''), "no single quotes");
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo \"10.0.0\"; exit 0; fi\n\
         if [ \"$2\" = \"skills\" ] && [ \"$3\" = \"ls\" ]; then echo '[]'; exit 0; fi\n\
         case \"$4\" in *#*) echo '{tag_failure}'; exit 1;; esac\n\
         echo \"$@\" > \"{}\"\n",
        args_file.display()
    );
    let npx = dir.join("npx");
    fs::write(&npx, script).unwrap();
    fs::set_permissions(&npx, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn install_falls_back_to_default_branch_when_tag_is_missing() {
    let home = temp_dir("tag_fallback");
    let bin = temp_dir("tag_fallback_bin");
    let args_file = bin.join("args.txt");
    // The installer's real output for a tag that does not exist.
    install_fake_npx_failing_tag(
        &bin,
        &args_file,
        "[SKILLS_GIT_CLONE_FAILED] Failed to clone https://github.com/coralogix/cx-cli.git: \
         fatal: Remote branch v0.0.999 not found in upstream origin",
    );

    cx(&home, &bin)
        .args(["skills", "install", "--global"])
        .assert()
        .success();

    let recorded = fs::read_to_string(&args_file).unwrap();
    assert_eq!(
        recorded.trim(),
        "-y skills add coralogix/cx-cli/skills --skill * -y -g"
    );
}

/// A network, auth or installer failure must not install unpinned skills.
#[cfg(unix)]
#[test]
fn install_does_not_fall_back_on_other_tag_failures() {
    let home = temp_dir("tag_no_fallback");
    let bin = temp_dir("tag_no_fallback_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx_failing_tag(
        &bin,
        &args_file,
        "[SKILLS_GIT_CLONE_TIMEOUT] Clone timed out after 60s",
    );

    cx(&home, &bin)
        .args(["skills", "install", "--global"])
        .assert()
        .failure();

    assert!(
        !args_file.exists(),
        "the default-branch install must not run"
    );
    assert_eq!(recorded_skills_version(&home), None);
}

#[cfg(unix)]
#[test]
fn global_install_records_version_and_agents() {
    let home = temp_dir("records_agents");
    let bin = temp_dir("records_agents_bin");
    install_fake_npx(&bin, &bin.join("args.txt"), "[]", "[]");

    cx(&home, &bin)
        .args(["skills", "install", "--global", "--agent", "cursor"])
        .assert()
        .success();

    let state = read_state(&home);
    assert_eq!(state["skills_installed_for"], env!("CARGO_PKG_VERSION"));
    assert_eq!(state["skills_agents"], serde_json::json!(["cursor"]));
}

#[cfg(unix)]
#[test]
fn outdated_global_skills_refresh_in_the_background() {
    let home = temp_dir("auto_refresh");
    let bin = temp_dir("auto_refresh_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file, "[]", "[]");
    fs::create_dir_all(home.join(".cx")).unwrap();
    fs::write(
        state_path(&home),
        r#"{"skills_installed_for":"0.0.1","skills_agents":["cursor"]}"#,
    )
    .unwrap();

    let output = cx(&home, &bin)
        .env_remove("CX_NO_UPDATE_NOTIFIER")
        .env_remove("CX_NO_SKILLS_AUTO_UPDATE")
        .arg("cleanup")
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("refreshing cx agent skills"),
        "stderr: {stderr}"
    );

    // The detached `cx skills auto-refresh` repeats the recorded install,
    // pinned to this version, then records it.
    assert!(
        wait_for(|| read_state(&home)["skills_installed_for"] == env!("CARGO_PKG_VERSION")),
        "refresh never recorded; state: {}",
        read_state(&home)
    );
    let recorded = fs::read_to_string(&args_file).unwrap();
    assert_eq!(
        recorded.trim(),
        format!(
            "-y skills add coralogix/cx-cli/skills#v{} --skill * -y -g -a cursor",
            env!("CARGO_PKG_VERSION")
        )
    );

    // Now current: the next command is silent.
    let output = cx(&home, &bin)
        .env_remove("CX_NO_UPDATE_NOTIFIER")
        .arg("cleanup")
        .output()
        .expect("failed to run cx");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("[cx update]"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn auto_refresh_opt_out_keeps_the_notice() {
    let home = temp_dir("auto_refresh_off");
    let bin = temp_dir("auto_refresh_off_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file, "[]", "[]");
    fs::create_dir_all(home.join(".cx")).unwrap();
    fs::write(state_path(&home), r#"{"skills_installed_for":"0.0.1"}"#).unwrap();
    fs::write(
        home.join(".cx").join("config.toml"),
        "auto_update_skills = false\n",
    )
    .unwrap();

    let output = cx(&home, &bin)
        .env_remove("CX_NO_UPDATE_NOTIFIER")
        .env_remove("CX_NO_SKILLS_AUTO_UPDATE")
        .arg("cleanup")
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cx agent skills are older than cx"),
        "stderr: {stderr}"
    );
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!args_file.exists(), "no install may run when opted out");
}

#[cfg(unix)]
#[test]
fn unrecorded_old_skills_refresh_with_auto_detected_agents() {
    // Skills from an older cx or plain `npx`: no version recorded, and the
    // installer's lock file dates them before this build.
    let home = temp_dir("auto_refresh_unknown");
    let bin = temp_dir("auto_refresh_unknown_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file, "[]", "[]");
    let lock_dir = home.join("state").join("skills");
    fs::create_dir_all(&lock_dir).unwrap();
    fs::write(
        lock_dir.join(".skill-lock.json"),
        r#"{"version":3,"skills":{"cx-alerts":{"source":"coralogix/cx-cli","updatedAt":"2020-01-01T00:00:00.000Z"}}}"#,
    )
    .unwrap();

    let output = cx(&home, &bin)
        .env("XDG_STATE_HOME", home.join("state"))
        .env_remove("CX_NO_UPDATE_NOTIFIER")
        .env_remove("CX_NO_SKILLS_AUTO_UPDATE")
        .arg("cleanup")
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("refreshing cx agent skills"),
        "stderr: {stderr}"
    );

    // No recorded agents: the installer auto-detects them (no `-a`).
    assert!(
        wait_for(|| read_state(&home)["skills_installed_for"] == env!("CARGO_PKG_VERSION")),
        "refresh never recorded; state: {}",
        read_state(&home)
    );
    let recorded = fs::read_to_string(&args_file).unwrap();
    assert_eq!(
        recorded.trim(),
        format!(
            "-y skills add coralogix/cx-cli/skills#v{} --skill * -y -g",
            env!("CARGO_PKG_VERSION")
        )
    );
}

/// Skills installed with plain `npx` leave no `~/.cx`; the refresh must still
/// take its lock and run instead of quietly exiting.
#[cfg(unix)]
#[test]
fn auto_refresh_runs_without_a_cx_config_dir() {
    let home = temp_dir("refresh_no_cx_dir");
    let bin = temp_dir("refresh_no_cx_dir_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file, "[]", "[]");
    assert!(!home.join(".cx").exists());

    cx(&home, &bin)
        .env("CX_NO_UPDATE_NOTIFIER", "1")
        .args(["skills", "auto-refresh"])
        .assert()
        .success();

    assert!(args_file.exists(), "the refresh install must run");
    assert!(
        !home.join(".cx").join("skills-refresh.lock").exists(),
        "the lock is released after the run"
    );
}
