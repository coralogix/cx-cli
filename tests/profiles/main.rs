use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_home() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cx_test_profiles_{pid}_{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn cx(home: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("CX_HOME", home);
    cmd
}

fn seed_profile(home: &std::path::Path, name: &str) {
    seed_profile_with_region(home, name, "eu1");
}

fn seed_profile_with_region(home: &std::path::Path, name: &str, region: &str) {
    let profiles_dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    let profile_toml = format!(
        r#"
auth = "api_key"
credential_storage = "file"
api_key = "fake-key-000"
region = "{region}"
"#
    );
    fs::write(profiles_dir.join(format!("{name}.toml")), profile_toml).unwrap();
}

fn seed_config(home: &std::path::Path, content: &str) {
    let cx_dir = home.join(".cx");
    fs::create_dir_all(&cx_dir).unwrap();
    fs::write(cx_dir.join("config.toml"), content).unwrap();
}

fn load_config_toml(home: &std::path::Path) -> String {
    fs::read_to_string(home.join(".cx").join("config.toml")).unwrap_or_default()
}

// ── Empty name rejection ─────────────────────────────────────────────────────

#[test]
fn add_empty_name_via_arg_is_rejected() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "add", ""])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Profile name cannot be empty"),
        "expected empty-name error, stderr: {stderr}"
    );
}

// ── --set-default flag ───────────────────────────────────────────────────────
// `profiles add` is interactive (prompts for auth), so we can't drive it
// end-to-end without a TTY. Instead we verify the flag is accepted by Clap
// and that `profiles set-default` (non-interactive) works correctly on a
// seeded profile.

#[test]
fn set_default_flag_is_accepted_by_clap() {
    let tmp = temp_home();
    // Verify clap registers the flag via --help. We can't drive the full
    // `profiles add` flow without a TTY, and on Windows the prompt code
    // (crossterm reading CONIN$ directly) hangs instead of failing fast
    // even with stdin redirected.
    let output = cx(&tmp)
        .args(["profiles", "add", "--help"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success(), "--help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--set-default"),
        "--set-default should appear in help, got: {stdout}"
    );
}

// ── --endpoint flag ──────────────────────────────────────────────────────────

#[test]
fn add_endpoint_flag_is_accepted_by_clap() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "add", "--help"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success(), "--help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--endpoint"),
        "--endpoint should appear in help, got: {stdout}"
    );
}

#[test]
fn add_invalid_endpoint_fails_before_prompting() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "add", "myenv", "--endpoint", "not-a-url"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid endpoint URL"),
        "should report invalid endpoint, stderr: {stderr}"
    );
}

// ── profiles show ────────────────────────────────────────────────────────────

#[test]
fn show_prints_resolved_endpoint_for_canned_region() {
    let tmp = temp_home();
    seed_profile(&tmp, "prod");
    seed_config(&tmp, "default_profile = \"prod\"\n");

    let output = cx(&tmp)
        .args(["profiles", "show", "prod"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("https://api.eu1.coralogix.com"),
        "should print the resolved endpoint, got: {stdout}"
    );
    assert!(
        !stdout.contains("fake-key-000"),
        "must never print credentials, got: {stdout}"
    );
}

#[test]
fn show_prints_custom_endpoint() {
    let tmp = temp_home();
    seed_profile_with_region(&tmp, "byoc", "https://api.myenv.example.com");
    seed_config(&tmp, "default_profile = \"byoc\"\n");

    let output = cx(&tmp)
        .args(["profiles", "show", "byoc"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("https://api.myenv.example.com"),
        "should print the custom endpoint, got: {stdout}"
    );
}

#[test]
fn show_defaults_to_default_profile() {
    let tmp = temp_home();
    seed_profile(&tmp, "primary");
    seed_config(&tmp, "default_profile = \"primary\"\n");

    let output = cx(&tmp)
        .args(["profiles", "show"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("primary"),
        "should show the default profile, got: {stdout}"
    );
}

#[test]
fn show_nonexistent_profile_fails() {
    let tmp = temp_home();
    seed_config(&tmp, "");

    let output = cx(&tmp)
        .args(["profiles", "show", "ghost"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "should report not found, stderr: {stderr}"
    );
}

// ── profiles set-default ─────────────────────────────────────────────────────

#[test]
fn set_default_updates_config() {
    let tmp = temp_home();
    seed_profile(&tmp, "staging");
    seed_config(&tmp, "default_profile = \"other\"\n");

    let output = cx(&tmp)
        .args(["profiles", "set-default", "staging"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());

    let config = load_config_toml(&tmp);
    assert!(
        config.contains("\"staging\""),
        "config should have staging as default, got: {config}"
    );
}

#[test]
fn set_default_nonexistent_profile_fails() {
    let tmp = temp_home();
    seed_config(&tmp, "");

    let output = cx(&tmp)
        .args(["profiles", "set-default", "nonexistent"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "should report profile not found, stderr: {stderr}"
    );
}

#[test]
fn set_default_already_default_is_noop() {
    let tmp = temp_home();
    seed_profile(&tmp, "prod");
    seed_config(&tmp, "default_profile = \"prod\"\n");

    let output = cx(&tmp)
        .args(["profiles", "set-default", "prod"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("already the default"),
        "should indicate already default, stdout: {stdout}"
    );
}

// ── profiles list ────────────────────────────────────────────────────────────

#[test]
fn list_shows_seeded_profiles() {
    let tmp = temp_home();
    seed_profile(&tmp, "alpha");
    seed_profile(&tmp, "beta");
    seed_config(&tmp, "default_profile = \"alpha\"\n");

    let output = cx(&tmp)
        .args(["profiles", "list"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("alpha"), "should list alpha, got: {stdout}");
    assert!(stdout.contains("beta"), "should list beta, got: {stdout}");
}

#[test]
fn list_empty_shows_hint() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "list"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No profiles configured"),
        "should show hint, got: {stdout}"
    );
}

// ── profiles delete ──────────────────────────────────────────────────────────

#[test]
fn delete_with_force_removes_profile() {
    let tmp = temp_home();
    seed_profile(&tmp, "disposable");
    seed_config(&tmp, "");

    let output = cx(&tmp)
        .args(["profiles", "delete", "disposable", "--force"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());

    let profile_path = tmp.join(".cx").join("profiles").join("disposable.toml");
    assert!(
        !profile_path.exists(),
        "profile file should be deleted after force delete"
    );
}

#[test]
fn delete_nonexistent_profile_fails() {
    let tmp = temp_home();
    seed_config(&tmp, "");

    let output = cx(&tmp)
        .args(["profiles", "delete", "ghost", "--force"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "should report not found, stderr: {stderr}"
    );
}
