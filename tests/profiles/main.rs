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
    // `profiles add` reads CX_API_KEY; a key exported in the user's shell
    // must not be used in tests.
    cmd.env_remove("CX_API_KEY");
    cmd
}

fn load_profile_toml(home: &std::path::Path, name: &str) -> String {
    fs::read_to_string(
        home.join(".cx")
            .join("profiles")
            .join(format!("{name}.toml")),
    )
    .unwrap_or_default()
}

fn seed_profile(home: &std::path::Path, name: &str) {
    let profiles_dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    let profile_toml = r#"
auth = "api_key"
credential_storage = "file"
api_key = "fake-key-000"
region = "eu1"
"#;
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

// ── profiles refresh ─────────────────────────────────────────────────────────
// The browser handshake itself can't be driven from a test, but every
// pre-flight branch fails before `browser_login` is reached, so all of them
// are checked here without opening a browser.

fn seed_oauth_profile(home: &std::path::Path, name: &str, extra: &str) {
    let profiles_dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    let profile_toml = format!(
        "auth = \"o_auth\"\n\
         credential_storage = \"file\"\n\
         {extra}"
    );
    fs::write(profiles_dir.join(format!("{name}.toml")), profile_toml).unwrap();
}

#[test]
fn refresh_nonexistent_profile_fails() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "refresh", "ghost"])
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
fn refresh_api_key_profile_is_rejected() {
    let tmp = temp_home();
    seed_profile(&tmp, "static");

    let output = cx(&tmp)
        .args(["profiles", "refresh", "static"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("API key authentication"),
        "should explain that API keys don't expire, stderr: {stderr}"
    );
    assert!(
        stderr.contains("cx profiles add static"),
        "should point at `profiles add` to change credentials, stderr: {stderr}"
    );
}

#[test]
fn refresh_custom_region_without_client_id_fails() {
    let tmp = temp_home();
    seed_oauth_profile(
        &tmp,
        "custom",
        "region = \"https://api.myenv.coralogix.com\"\n",
    );

    let output = cx(&tmp)
        .args(["profiles", "refresh", "custom"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No OAuth client ID configured"),
        "should report the missing client ID, stderr: {stderr}"
    );
}

#[test]
fn refresh_leaves_profile_untouched_on_failure() {
    let tmp = temp_home();
    let original = "auth = \"o_auth\"\n\
                    credential_storage = \"file\"\n\
                    region = \"stg1\"\n\
                    label = \"staging\"\n\
                    default_tier = \"frequent\"\n";
    let profiles_dir = tmp.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    fs::write(profiles_dir.join("staging.toml"), original).unwrap();

    // stg1 has no built-in OAuth client ID, so this fails pre-flight.
    let output = cx(&tmp)
        .args(["profiles", "refresh", "staging"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());

    let after = fs::read_to_string(profiles_dir.join("staging.toml")).unwrap();
    assert_eq!(
        after, original,
        "a failed refresh must not rewrite the profile"
    );
}

// ── re-authentication hint on stderr ─────────────────────────────────────────

#[test]
fn expired_oauth_profile_is_not_also_told_to_run_profiles_add() {
    // An OAuth profile with no stored tokens fails in `oauth::resolve_token`
    // with its own `cx profiles refresh` instruction. The generic
    // "Run `cx profiles add` to set up credentials." fallback must not be
    // printed underneath it - two contradictory instructions on one stderr is
    // what `profiles refresh` exists to fix.
    let tmp = temp_home();
    seed_oauth_profile(&tmp, "expired", "region = \"eu2\"\n");

    let output = cx(&tmp)
        .args(["-p", "expired", "alerts", "list"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cx profiles refresh expired"),
        "should point at `profiles refresh`, stderr: {stderr}"
    );
    assert!(
        !stderr.contains("cx profiles add"),
        "should not also suggest `profiles add`, stderr: {stderr}"
    );
}

#[test]
fn refresh_appears_in_profiles_help() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "--help"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("refresh"),
        "refresh should be listed as a profiles subcommand, got: {stdout}"
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

// ── profiles add: non-interactive (no TTY) ───────────────────────────────────
// The test harness runs the binary without a terminal, so `profiles add`
// takes the non-interactive path: flags answer questions, missing required
// values are errors, and nothing hangs waiting for input.

#[test]
fn add_non_interactive_with_flags_creates_default_profile() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "--api-key",
            "test-key-123",
            "--region",
            "eu2",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");

    let profile = load_profile_toml(&tmp, "default");
    assert!(
        profile.contains("\"eu2\""),
        "profile should record region eu2, got: {profile}"
    );
    assert!(
        profile.contains("test-key-123"),
        "profile should store the API key in the file, got: {profile}"
    );
    assert!(
        profile.contains("api_key"),
        "profile should use api-key auth, got: {profile}"
    );

    // First profile becomes the default automatically.
    let config = load_config_toml(&tmp);
    assert!(
        config.contains("\"default\""),
        "config should set 'default' as default profile, got: {config}"
    );
}

#[test]
fn add_non_interactive_missing_region_fails_with_flag_hint() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "add", "--api-key", "k"])
        .output()
        .expect("failed to run cx");
    assert!(
        !output.status.success(),
        "missing region must not hang or succeed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pass --url or --region"),
        "error should name the exact flags to add, stderr: {stderr}"
    );
}

#[test]
fn add_non_interactive_missing_api_key_fails_with_flag_hint() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "add", "--region", "eu2"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pass --api-key or set CX_API_KEY"),
        "error should name the exact flag to add, stderr: {stderr}"
    );
}

#[test]
fn add_non_interactive_missing_everything_reports_all_missing_values() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "add"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pass --url or --region") && stderr.contains("pass --api-key"),
        "error should list every missing value, stderr: {stderr}"
    );
}

/// An empty `--api-key ""` (e.g. a CI secret expanding to nothing) must be
/// treated as missing — not silently saved as an unusable profile.
#[test]
fn add_non_interactive_empty_api_key_is_treated_as_missing() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "add", "--region", "eu2", "--api-key", ""])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success(), "empty API key must be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pass --api-key or set CX_API_KEY"),
        "error should name the exact flag to add, stderr: {stderr}"
    );
}

/// A whitespace-only key is just as unusable as an empty one.
#[test]
fn add_non_interactive_whitespace_api_key_is_treated_as_missing() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .env("CX_API_KEY", "   ")
        .args(["profiles", "add", "--region", "eu2"])
        .output()
        .expect("failed to run cx");
    assert!(
        !output.status.success(),
        "whitespace-only API key must be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pass --api-key or set CX_API_KEY"),
        "error should name the exact flag to add, stderr: {stderr}"
    );
}

#[test]
fn add_honors_cx_api_key_env() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .env("CX_API_KEY", "env-key-456")
        .args(["profiles", "add", "--region", "us1"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let profile = load_profile_toml(&tmp, "default");
    assert!(
        profile.contains("env-key-456"),
        "profile should store the key from CX_API_KEY, got: {profile}"
    );
}

// ── --oauth flag ─────────────────────────────────────────────────────────────

/// Non-interactive `--oauth` works (the sign-in URL is printed, approval
/// happens in the browser), but the region has no default: without a terminal
/// to prompt on, missing --url/--region is an error before any login starts.
#[test]
fn add_oauth_non_interactive_without_region_fails_with_flag_hint() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "add", "--oauth"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pass --url or --region"),
        "expected missing-region error naming the flags, stderr: {stderr}"
    );
    assert!(
        load_profile_toml(&tmp, "default").is_empty(),
        "no profile should be written on failure"
    );
}

/// `--oauth` must win over an exported CX_API_KEY: with a custom endpoint the
/// OAuth path fails on the missing client ID, while the API-key path would
/// have silently saved a profile from the env value.
#[test]
fn add_oauth_wins_over_env_api_key() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .env("CX_API_KEY", "env-key-456")
        .args([
            "profiles",
            "add",
            "--oauth",
            "--url",
            "https://api.myenv.internal",
        ])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("OAuth client ID"),
        "expected the OAuth path's client-ID error, not the API-key path, stderr: {stderr}"
    );
    assert!(
        load_profile_toml(&tmp, "default").is_empty(),
        "no API-key profile should be written when --oauth is passed"
    );
}

#[test]
fn add_url_flag_derives_region() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "--url",
            "https://myteam.app.cx498.coralogix.com/logs",
            "--api-key",
            "k",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let profile = load_profile_toml(&tmp, "default");
    // cx498 is US2's app-domain cluster label — the crux derivation case.
    assert!(
        profile.contains("\"us2\""),
        "profile should record derived region us2, got: {profile}"
    );
}

#[test]
fn add_url_flag_unresolved_becomes_custom_endpoint() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "--url",
            "https://api.myenv.internal/",
            "--api-key",
            "k",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let profile = load_profile_toml(&tmp, "default");
    assert!(
        profile.contains("https://api.myenv.internal"),
        "profile should record the custom endpoint, got: {profile}"
    );
}

#[test]
fn add_unknown_region_flag_is_rejected() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "add", "--api-key", "k", "--region", "eu22"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown region 'eu22'"),
        "should reject a typo'd region instead of writing it, stderr: {stderr}"
    );
}

#[test]
fn add_url_conflicts_with_region() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "--url",
            "https://myteam.coralogix.com",
            "--region",
            "eu1",
            "--api-key",
            "k",
        ])
        .output()
        .expect("failed to run cx");
    assert!(
        !output.status.success(),
        "--url and --region are mutually exclusive"
    );
}

// ── profiles add: --profile flag ─────────────────────────────────────────────

#[test]
fn add_name_flag_names_the_profile() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "--name",
            "prod",
            "--api-key",
            "k",
            "--region",
            "eu1",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    assert!(
        tmp.join(".cx").join("profiles").join("prod.toml").exists(),
        "profile 'prod' should be created"
    );
}

#[test]
fn add_name_flag_conflicts_with_positional_name() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "prod",
            "--name",
            "other",
            "--api-key",
            "k",
            "--region",
            "eu1",
        ])
        .output()
        .expect("failed to run cx");
    assert!(
        !output.status.success(),
        "positional NAME and --name are mutually exclusive"
    );
}

// ── profiles add: existing-profile guard ─────────────────────────────────────

#[test]
fn add_existing_profile_non_interactive_fails_without_force() {
    let tmp = temp_home();
    seed_profile(&tmp, "default");
    seed_config(&tmp, "default_profile = \"default\"\n");

    let output = cx(&tmp)
        .args(["profiles", "add", "--api-key", "new-key", "--region", "eu2"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("profile 'default' already exists"),
        "should refuse to overwrite, stderr: {stderr}"
    );
    let profile = load_profile_toml(&tmp, "default");
    assert!(
        profile.contains("fake-key-000"),
        "existing profile must be untouched, got: {profile}"
    );
}

#[test]
fn add_existing_profile_with_force_overwrites() {
    let tmp = temp_home();
    seed_profile(&tmp, "default");
    seed_config(&tmp, "default_profile = \"default\"\n");

    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "--api-key",
            "new-key",
            "--region",
            "eu2",
            "--force",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let profile = load_profile_toml(&tmp, "default");
    assert!(
        profile.contains("new-key") && profile.contains("\"eu2\""),
        "profile should be overwritten, got: {profile}"
    );
}

// ── profiles add: --set-default non-interactive ──────────────────────────────

#[test]
fn add_second_profile_with_set_default_updates_config() {
    let tmp = temp_home();
    seed_profile(&tmp, "first");
    seed_config(&tmp, "default_profile = \"first\"\n");

    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "second",
            "--api-key",
            "k",
            "--region",
            "eu1",
            "--set-default",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let config = load_config_toml(&tmp);
    assert!(
        config.contains("\"second\""),
        "config should switch default to 'second', got: {config}"
    );
}

#[test]
fn add_second_profile_without_set_default_keeps_existing_default() {
    let tmp = temp_home();
    seed_profile(&tmp, "first");
    seed_config(&tmp, "default_profile = \"first\"\n");

    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "second",
            "--api-key",
            "k",
            "--region",
            "eu1",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let config = load_config_toml(&tmp);
    assert!(
        config.contains("\"first\""),
        "default profile must stay 'first', got: {config}"
    );
}
