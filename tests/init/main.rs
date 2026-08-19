//! Integration tests for `cx init` (FORGE-658).
//!
//! `cx init` chains `cx profiles add` and `cx skills install`. These tests
//! drive the non-interactive (advanced) path — the coding-agent one-liner — so
//! no terminal, Node.js, or network is required: the skills installer is a fake
//! `npx` on PATH that records its arguments, exactly as the skills tests do.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cx_test_init_{tag}_{pid}_{id}"));
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
        // Never inherit the developer's key into the sandbox.
        .env_remove("CX_API_KEY")
        .env_remove("CX_REGION")
        .env_remove("CX_PROFILE")
        .current_dir(cwd);
    cmd
}

/// Install a fake `npx` that answers `--version`, serves `[]` for
/// `skills ls --json` (nothing already installed), and records any other
/// invocation's arguments into `args_file`.
#[cfg(unix)]
fn install_fake_npx(dir: &Path, args_file: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo \"10.0.0\"; exit 0; fi\n\
         if [ \"$2\" = \"skills\" ] && [ \"$3\" = \"ls\" ]; then echo '[]'; exit 0; fi\n\
         echo \"$@\" > \"{}\"\n",
        args_file.display()
    );
    let npx = dir.join("npx");
    fs::write(&npx, script).unwrap();
    fs::set_permissions(&npx, fs::Permissions::from_mode(0o755)).unwrap();
}

fn profile_path(home: &Path, name: &str) -> PathBuf {
    home.join(".cx")
        .join("profiles")
        .join(format!("{name}.toml"))
}

// ── Advanced (non-interactive) profile + skills chain ─────────────────────────

#[cfg(unix)]
#[test]
fn init_writes_profile_then_installs_skills() {
    let home = temp_dir("chain");
    let bin = temp_dir("chain_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file);

    cx(&home, &bin)
        .args([
            "init",
            "--url",
            "https://team.app.us1.coralogix.com",
            "--api-key",
            "faketoken",
            "--global",
        ])
        .assert()
        .success();

    // Step 1: profile written with the quick-setup defaults (file + json).
    let toml = fs::read_to_string(profile_path(&home, "default")).unwrap();
    assert!(toml.contains("auth = \"api_key\""), "profile: {toml}");
    assert!(toml.contains("region = \"us1\""), "profile: {toml}");
    assert!(
        toml.contains("credential_storage = \"file\""),
        "quick setup pins file storage: {toml}"
    );
    assert!(
        toml.contains("default_output_format = \"json\""),
        "quick setup pins json output: {toml}"
    );

    // Step 2: the skills installer ran with the fully non-interactive args.
    let recorded = fs::read_to_string(&args_file).unwrap();
    assert_eq!(
        recorded.trim(),
        "-y skills add coralogix/cx-cli/skills --skill * -y -g"
    );
}

#[cfg(unix)]
#[test]
fn init_names_profile_from_profile_flag() {
    let home = temp_dir("named");
    let bin = temp_dir("named_bin");
    install_fake_npx(&bin, &bin.join("args.txt"));

    cx(&home, &bin)
        .args([
            "init",
            "--profile",
            "prod",
            "--region",
            "eu2",
            "--api-key",
            "k",
            "--local",
        ])
        .assert()
        .success();

    assert!(
        profile_path(&home, "prod").exists(),
        "profile should be written under the --profile name"
    );
}

#[cfg(unix)]
#[test]
fn init_agents_flag_reaches_the_installer() {
    let home = temp_dir("agents");
    let bin = temp_dir("agents_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file);

    cx(&home, &bin)
        .args([
            "init",
            "--region",
            "eu2",
            "--api-key",
            "k",
            "--global",
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
        "-y skills add coralogix/cx-cli/skills --skill * -y -g -a claude-code cursor"
    );
}

// ── --no-skills skips the installer entirely ──────────────────────────────────

#[cfg(unix)]
#[test]
fn init_no_skills_skips_the_installer() {
    let home = temp_dir("noskills");
    let bin = temp_dir("noskills_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file);

    cx(&home, &bin)
        .args(["init", "--region", "eu2", "--api-key", "k", "--no-skills"])
        .assert()
        .success();

    assert!(
        profile_path(&home, "default").exists(),
        "profile is still written"
    );
    assert!(
        !args_file.exists(),
        "the skills installer must not run with --no-skills"
    );
}

// ── Skills obstacles never brick onboarding ───────────────────────────────────

/// A non-interactive run without `--global`/`--local` cannot resolve a scope,
/// so the skills step fails inside the skills command — but init downgrades
/// that to a warning and still succeeds, having written the profile.
#[cfg(unix)]
#[test]
fn init_without_scope_warns_but_succeeds() {
    let home = temp_dir("noscope");
    let bin = temp_dir("noscope_bin");
    install_fake_npx(&bin, &bin.join("args.txt"));

    let output = cx(&home, &bin)
        .args(["init", "--region", "eu2", "--api-key", "k"])
        .output()
        .expect("failed to run cx");

    assert!(
        output.status.success(),
        "init must succeed even when skills is skipped"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("skipped the agent-skills install"),
        "expected a skills-skipped warning, stderr: {stderr}"
    );
    assert!(
        profile_path(&home, "default").exists(),
        "profile is written"
    );
}

/// npx missing → the skills step fails, but onboarding still succeeds.
#[cfg(unix)]
#[test]
fn init_without_npx_warns_but_succeeds() {
    let home = temp_dir("nonpx");
    let empty_path = temp_dir("nonpx_path"); // no npx on PATH

    let output = cx(&home, &empty_path)
        .args(["init", "--region", "eu2", "--api-key", "k", "--global"])
        .output()
        .expect("failed to run cx");

    assert!(output.status.success(), "init must not brick without npx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("skipped the agent-skills install") && stderr.contains("Node.js"),
        "expected a Node.js-related skills warning, stderr: {stderr}"
    );
    assert!(
        profile_path(&home, "default").exists(),
        "profile is written"
    );
}

// ── Idempotency: an existing profile is left untouched ────────────────────────

/// Re-running `cx init` when a profile already exists must skip the profile
/// step (leaving the existing profile untouched) and still run the skills
/// install. This is what makes `cx init` safe to re-run.
#[cfg(unix)]
#[test]
fn init_skips_profile_setup_when_one_already_exists() {
    let home = temp_dir("idempotent");
    let bin = temp_dir("idempotent_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file);

    // First run creates the default profile (region us1).
    cx(&home, &bin)
        .args([
            "init",
            "--url",
            "https://team.app.us1.coralogix.com",
            "--api-key",
            "first",
            "--global",
        ])
        .assert()
        .success();
    let _ = fs::remove_file(&args_file); // so we can prove skills ran the 2nd time

    // Second run supplies different credentials/region, but the existing
    // profile must win: init skips profile setup entirely.
    let output = cx(&home, &bin)
        .args(["init", "--region", "eu2", "--api-key", "second", "--global"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("already configured") && stdout.contains("skipping"),
        "expected a skip notice on the second run, stdout: {stdout}"
    );

    // The profile is unchanged — still region us1 with the first key.
    let toml = fs::read_to_string(profile_path(&home, "default")).unwrap();
    assert!(
        toml.contains("region = \"us1\"") && toml.contains("first"),
        "existing profile must not be overwritten: {toml}"
    );

    // Skills still ran on the second invocation.
    assert!(
        args_file.exists(),
        "the skills installer should still run when profile setup is skipped"
    );
}

/// When cx skills are already installed, init skips the skills step (idempotent)
/// rather than reinstalling — updating is `cx skills install`'s job.
#[cfg(unix)]
#[test]
fn init_skips_skills_when_already_installed() {
    use std::os::unix::fs::PermissionsExt;
    let home = temp_dir("skills_present");
    let bin = temp_dir("skills_present_bin");
    let args_file = bin.join("args.txt");

    // Fake npx reports a cx skill already installed; records `add` args if run.
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo \"10.0.0\"; exit 0; fi\n\
         if [ \"$2\" = \"skills\" ] && [ \"$3\" = \"ls\" ]; then \
           echo '[{{\"name\":\"cx-alerts\",\"source\":\"coralogix/cx-cli\"}}]'; exit 0; fi\n\
         echo \"$@\" > \"{}\"\n",
        args_file.display()
    );
    let npx = bin.join("npx");
    fs::write(&npx, script).unwrap();
    fs::set_permissions(&npx, fs::Permissions::from_mode(0o755)).unwrap();

    let output = cx(&home, &bin)
        .args(["init", "--region", "eu2", "--api-key", "k", "--global"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("already installed") && stdout.contains("skipping"),
        "expected the skills step to be skipped, stdout: {stdout}"
    );
    assert!(
        !args_file.exists(),
        "the skills installer must not run when skills are already present"
    );
}

// ── Missing required profile values fail actionably ───────────────────────────

#[test]
fn init_without_region_fails_naming_the_flags() {
    let home = temp_dir("noregion");
    let bin = temp_dir("noregion_bin");

    let output = cx(&home, &bin)
        .args(["init", "--api-key", "k", "--no-skills"])
        .output()
        .expect("failed to run cx");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--url") || stderr.contains("--region"),
        "expected an error naming --url/--region, stderr: {stderr}"
    );
}
