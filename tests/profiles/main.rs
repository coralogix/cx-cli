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

fn profile_labels(home: &std::path::Path, name: &str) -> Vec<String> {
    let raw = load_profile_toml(home, name);
    let profile: coralogix_cli::config::Profile =
        toml::from_str(&raw).unwrap_or_else(|err| panic!("parse {name}: {err}\n{raw}"));
    profile.labels
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

#[test]
fn expired_oauth_profile_prints_resolution_error_once() {
    let tmp = temp_home();
    let profiles_dir = tmp.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    fs::write(
        profiles_dir.join("oauth-expired.toml"),
        r#"
auth = "o_auth"
credential_storage = "file"
region = "eu2"

[oauth_tokens]
access_token = "expired"
expiry = 1
"#,
    )
    .unwrap();

    let output = cx(&tmp)
        .env_remove("CX_REGION")
        .env_remove("CX_PROFILE")
        .args(["-p", "oauth-expired", "logs", "source logs | limit 1"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "should fail, stderr: {stderr}");
    assert_eq!(
        stderr.matches("OAuth session expired").count(),
        1,
        "resolution error must be printed once: {stderr}"
    );
    assert!(
        stderr.contains("cx profiles refresh oauth-expired"),
        "stderr should name the profile to re-authenticate: {stderr}"
    );
    assert!(
        !stderr.contains("Error:") && !stderr.contains("set up credentials"),
        "stderr must not repeat the error or add a second instruction: {stderr}"
    );
}

// ── Refresh ──────────────────────────────────────────────────────────────────

#[test]
fn refresh_is_listed_in_profiles_help() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "--help"])
        .output()
        .expect("failed to run cx");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "help should succeed: {stdout}");
    assert!(
        stdout.contains("refresh") && stdout.contains("Sign an OAuth profile in again"),
        "profiles help should list refresh: {stdout}"
    );
}

#[test]
fn refresh_missing_profile_points_to_add() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "refresh", "nope"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "should fail, stderr: {stderr}");
    assert!(
        stderr.contains("Profile 'nope' not found") && stderr.contains("cx profiles add nope"),
        "stderr should say the profile is missing and how to create it: {stderr}"
    );
}

#[test]
fn refresh_api_key_profile_is_rejected_and_left_unchanged() {
    let tmp = temp_home();
    seed_profile(&tmp, "keyed");
    let before = load_profile_toml(&tmp, "keyed");
    let output = cx(&tmp)
        .args(["profiles", "refresh", "keyed"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "should fail, stderr: {stderr}");
    assert!(
        stderr.contains("uses an API key") && stderr.contains("cx profiles add keyed"),
        "stderr should explain there is no sign-in to refresh: {stderr}"
    );
    assert_eq!(
        load_profile_toml(&tmp, "keyed"),
        before,
        "an API-key profile must not be rewritten"
    );
}

/// An OAuth profile created before the team was recorded at sign-in learns it
/// on the next command while its token is still valid, so its first
/// expired-session re-login can narrow the consent page to that team.
#[tokio::test]
async fn oauth_profile_without_team_records_it_while_token_is_valid() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/identity/whoami"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "team_id": 12,
            "team_name": "c4c",
            "team_url": "https://c4c.app.eu2.coralogix.com"
        })))
        .mount(&server)
        .await;

    let tmp = temp_home();
    let profiles_dir = tmp.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    fs::write(
        profiles_dir.join("oauth-noteam.toml"),
        format!(
            r#"
auth = "o_auth"
credential_storage = "file"
region = "{}"
oauth_client_id = "test-client"

[oauth_tokens]
access_token = "valid"
expiry = 4102444800
"#,
            server.uri()
        ),
    )
    .unwrap();

    // The command itself may fail against the mock; only the backfill matters.
    let _ = cx(&tmp)
        .env_remove("CX_REGION")
        .env_remove("CX_PROFILE")
        .args(["-p", "oauth-noteam", "alerts", "list"])
        .output()
        .expect("failed to run cx");

    let toml = load_profile_toml(&tmp, "oauth-noteam");
    assert!(
        toml.contains("cached_team_id = 12"),
        "team id should be recorded: {toml}"
    );
    assert!(
        toml.contains("cached_team_name = \"c4c\""),
        "team name should be recorded: {toml}"
    );
}

/// The team backfill runs before the command and outside `--http-timeout`, so
/// a stalled `/identity/whoami` must hit its own short deadline instead of
/// holding the command up.
#[tokio::test]
async fn oauth_team_backfill_gives_up_on_a_stalled_whoami() {
    use std::time::{Duration, Instant};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/identity/whoami"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "team_id": 12, "team_name": "c4c" }))
                .set_delay(Duration::from_secs(60)),
        )
        .mount(&server)
        .await;

    let tmp = temp_home();
    let profiles_dir = tmp.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    fs::write(
        profiles_dir.join("oauth-stalled.toml"),
        format!(
            r#"
auth = "o_auth"
credential_storage = "file"
region = "{}"
oauth_client_id = "test-client"

[oauth_tokens]
access_token = "valid"
expiry = 4102444800
"#,
            server.uri()
        ),
    )
    .unwrap();

    let started = Instant::now();
    let _ = cx(&tmp)
        .env_remove("CX_REGION")
        .env_remove("CX_PROFILE")
        .args(["-p", "oauth-stalled", "alerts", "list"])
        .output()
        .expect("failed to run cx");
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(20),
        "a stalled whoami must not hold up the command (took {elapsed:?})"
    );
    let toml = load_profile_toml(&tmp, "oauth-stalled");
    assert!(
        !toml.contains("cached_team_id"),
        "a timed-out lookup must not record a team: {toml}"
    );
}

// ── Profile labels ───────────────────────────────────────────────────────────

fn write_profile(home: &std::path::Path, name: &str, body: &str) {
    let profiles_dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    fs::write(profiles_dir.join(format!("{name}.toml")), body).unwrap();
}

fn table_cells(stdout: &str) -> Vec<Vec<String>> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.split("  ")
                .filter(|cell| !cell.is_empty())
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

#[test]
fn add_sets_labels_from_repeatable_and_comma_flags() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "demo",
            "--api-key",
            "k",
            "--region",
            "eu1",
            "--label",
            " prod, eu ",
            "--label",
            "eu",
            "--label",
            "staging",
            "--label",
            "prod",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    assert_eq!(
        profile_labels(&tmp, "demo"),
        vec!["prod".to_string(), "eu".to_string(), "staging".to_string()]
    );
    let raw = load_profile_toml(&tmp, "demo");
    assert!(
        raw.contains("labels"),
        "new profiles should store a labels array, got: {raw}"
    );
    assert!(
        raw.lines()
            .all(|line| !line.trim_start().starts_with("label ")),
        "new profiles should not write the legacy label key, got: {raw}"
    );
}

#[test]
fn add_without_label_keeps_existing_labels() {
    let tmp = temp_home();
    write_profile(
        &tmp,
        "legacy",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"old\"\nregion = \"eu1\"\nlabel = \"keep-me\"\n",
    );
    write_profile(
        &tmp,
        "current",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"old\"\nregion = \"eu1\"\nlabels = [\"prod\", \"eu\"]\n",
    );
    seed_config(&tmp, "default_profile = \"legacy\"\n");

    for name in ["legacy", "current"] {
        let output = cx(&tmp)
            .args([
                "profiles",
                "add",
                name,
                "--api-key",
                "new-key",
                "--region",
                "eu2",
                "--force",
            ])
            .output()
            .expect("failed to run cx");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "{name} should reconfigure, stderr: {stderr}"
        );
    }

    assert_eq!(profile_labels(&tmp, "legacy"), vec!["keep-me".to_string()]);
    assert_eq!(
        profile_labels(&tmp, "current"),
        vec!["prod".to_string(), "eu".to_string()]
    );
    let legacy_raw = load_profile_toml(&tmp, "legacy");
    assert!(
        legacy_raw.contains("new-key") && legacy_raw.contains("labels"),
        "reconfigure should rewrite credentials and keep labels, got: {legacy_raw}"
    );
}

#[test]
fn add_label_flag_replaces_existing_labels() {
    let tmp = temp_home();
    write_profile(
        &tmp,
        "demo",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"old\"\nregion = \"eu1\"\nlabels = [\"prod\", \"eu\"]\n",
    );
    seed_config(&tmp, "default_profile = \"demo\"\n");

    let output = cx(&tmp)
        .args([
            "profiles",
            "add",
            "demo",
            "--api-key",
            "k",
            "--region",
            "eu1",
            "--force",
            "--label",
            "staging",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    assert_eq!(profile_labels(&tmp, "demo"), vec!["staging".to_string()]);
}

#[test]
fn list_shows_labels_and_adapts_column_width() {
    let tmp = temp_home();
    let joined = "production,europe-west";
    write_profile(
        &tmp,
        "wide",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\nlabels = [\"production\", \"europe-west\"]\n",
    );
    write_profile(
        &tmp,
        "plain",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\n",
    );
    write_profile(
        &tmp,
        "old",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu2\"\nlabel = \"legacy\"\n",
    );
    seed_config(&tmp, "default_profile = \"plain\"\n");

    let output = cx(&tmp)
        .args(["profiles", "list"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let rows = table_cells(&stdout);
    assert!(
        rows.len() >= 5,
        "expected header, rule, and 3 profiles, got: {stdout}"
    );

    let header = &rows[0];
    assert_eq!(header.first().map(String::as_str), Some("NAME"));
    assert_eq!(header.get(1).map(String::as_str), Some("LABEL"));

    // The rule row is dashes. Its LABEL cell must be as wide as the longest
    // joined value, which is longer than the "LABEL" header.
    let rule = &rows[1];
    assert_eq!(
        rule.get(1).map(String::len),
        Some(joined.len()),
        "LABEL column should grow to fit {joined}, table:\n{stdout}"
    );
    assert!(joined.len() > "LABEL".len());

    let mut labels_by_name = std::collections::HashMap::new();
    for row in rows.iter().skip(2) {
        let name = row.first().map(String::as_str).unwrap_or("");
        let label = row.get(1).map(String::as_str).unwrap_or("");
        labels_by_name.insert(name.to_string(), label.to_string());
    }
    assert_eq!(labels_by_name.get("wide").map(String::as_str), Some(joined));
    assert_eq!(labels_by_name.get("plain").map(String::as_str), Some("-"));
    assert_eq!(
        labels_by_name.get("old").map(String::as_str),
        Some("legacy"),
        "legacy label key should still display, table:\n{stdout}"
    );
}

#[test]
fn list_filter_matches_any_label() {
    let tmp = temp_home();
    write_profile(
        &tmp,
        "prod-eu",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\nlabels = [\"prod\", \"eu\"]\n",
    );
    write_profile(
        &tmp,
        "staging",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\nlabels = [\"staging\"]\n",
    );
    write_profile(
        &tmp,
        "bare",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\n",
    );

    let any = cx(&tmp)
        .args([
            "profiles", "list", "--label", " prod ", "--label", "staging",
        ])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&any.stderr);
    assert!(any.status.success(), "should succeed, stderr: {stderr}");
    let stdout = String::from_utf8_lossy(&any.stdout);
    assert!(
        stdout.contains("prod-eu"),
        "prod should match, got: {stdout}"
    );
    assert!(
        stdout.contains("staging"),
        "staging should match any of the flags, got: {stdout}"
    );
    assert!(
        !stdout.contains("bare"),
        "a profile with no labels should be excluded, got: {stdout}"
    );

    let comma = cx(&tmp)
        .args(["profiles", "list", "--label", "eu,staging"])
        .output()
        .expect("failed to run cx");
    let stdout = String::from_utf8_lossy(&comma.stdout);
    assert!(comma.status.success());
    assert!(
        stdout.contains("prod-eu") && stdout.contains("staging"),
        "{stdout}"
    );
    assert!(!stdout.contains("bare"), "{stdout}");
}

#[test]
fn list_filter_with_no_matches_is_explicit() {
    let tmp = temp_home();
    write_profile(
        &tmp,
        "demo",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\nlabels = [\"prod\"]\n",
    );

    let output = cx(&tmp)
        .args(["profiles", "list", "--label", "missing", "--label", "other"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "empty filter is not an error, stderr: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No profiles with label 'missing' or 'other'."),
        "empty filter should say so, got: {stdout}"
    );
    assert!(
        !stdout.contains("demo"),
        "non-matching profile should be hidden: {stdout}"
    );
}

#[test]
fn list_help_documents_any_label_match() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "list", "--help"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--label") && stdout.to_ascii_lowercase().contains("any"),
        "help should document that repeated --label matches any label, got: {stdout}"
    );
}

#[test]
fn refresh_leaves_labeled_api_key_profile_unchanged() {
    let tmp = temp_home();
    write_profile(
        &tmp,
        "keyed",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\nlabel = \"prod\"\nlabels = [\"eu\"]\n",
    );
    let before = load_profile_toml(&tmp, "keyed");
    let output = cx(&tmp)
        .args(["profiles", "refresh", "keyed"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success());
    assert_eq!(
        load_profile_toml(&tmp, "keyed"),
        before,
        "refresh must not rewrite labels when it refuses an API-key profile"
    );
}

// ── JSON output ──────────────────────────────────────────────────────────────

#[test]
fn list_json_includes_all_required_fields() {
    let tmp = temp_home();
    write_profile(
        &tmp,
        "demo",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"secret-key-123\"\nregion = \"eu2\"\nlabels = [\"prod\", \"eu\"]\ndefault_output_format = \"json\"\n",
    );
    write_profile(
        &tmp,
        "plain",
        "auth = \"o_auth\"\ncredential_storage = \"file\"\nregion = \"us1\"\n",
    );
    seed_config(&tmp, "default_profile = \"demo\"\n");

    let output = cx(&tmp)
        .args(["profiles", "list", "--output", "json"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let profiles: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).expect("should be valid JSON");
    assert_eq!(profiles.len(), 2, "should have 2 profiles");

    let demo = profiles
        .iter()
        .find(|p| p["name"] == "demo")
        .expect("demo profile should be present");
    assert_eq!(demo["name"], "demo");
    assert_eq!(demo["labels"], serde_json::json!(["prod", "eu"]));
    assert_eq!(demo["region"], "eu2");
    assert_eq!(demo["auth"], "api-key");
    assert_eq!(demo["output"], "json");
    assert_eq!(demo["default"], true);

    let plain = profiles
        .iter()
        .find(|p| p["name"] == "plain")
        .expect("plain profile should be present");
    assert_eq!(plain["name"], "plain");
    assert_eq!(plain["labels"], serde_json::json!([]));
    assert_eq!(plain["region"], "us1");
    assert_eq!(plain["auth"], "oauth");
    assert_eq!(plain["output"], serde_json::Value::Null);
    assert_eq!(plain["default"], false);
}

#[test]
fn list_json_excludes_secrets() {
    let tmp = temp_home();
    write_profile(
        &tmp,
        "keyed",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"secret-key-123\"\nregion = \"eu1\"\n",
    );
    write_profile(
        &tmp,
        "oauth",
        "auth = \"o_auth\"\ncredential_storage = \"file\"\nregion = \"us1\"\n\n[oauth_tokens]\naccess_token = \"secret-token-456\"\nexpiry = 4102444800\n",
    );

    let output = cx(&tmp)
        .args(["profiles", "list", "--output", "json"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("secret-key-123"),
        "API key should not appear in JSON output: {stdout}"
    );
    assert!(
        !stdout.contains("secret-token-456"),
        "OAuth token should not appear in JSON output: {stdout}"
    );
    assert!(
        !stdout.contains("api_key") || !stdout.contains("oauth_tokens"),
        "Secret field names should not appear in JSON output: {stdout}"
    );

    let profiles: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).expect("should be valid JSON");
    for profile in &profiles {
        let obj = profile.as_object().expect("profile should be an object");
        assert!(
            !obj.contains_key("api_key"),
            "api_key field should not be present"
        );
        assert!(
            !obj.contains_key("oauth_tokens"),
            "oauth_tokens field should not be present"
        );
    }
}

#[test]
fn list_json_with_label_filter_applies() {
    let tmp = temp_home();
    write_profile(
        &tmp,
        "prod-eu",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\nlabels = [\"prod\", \"eu\"]\n",
    );
    write_profile(
        &tmp,
        "staging",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\nlabels = [\"staging\"]\n",
    );
    write_profile(
        &tmp,
        "bare",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"us1\"\n",
    );

    let output = cx(&tmp)
        .args(["profiles", "list", "--label", "prod", "--output", "json"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let profiles: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).expect("should be valid JSON");

    assert_eq!(profiles.len(), 1, "should have 1 profile");
    assert_eq!(profiles[0]["name"], "prod-eu");
}

#[test]
fn list_json_empty_result_prints_empty_array() {
    let tmp = temp_home();
    write_profile(
        &tmp,
        "demo",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\nlabels = [\"prod\"]\n",
    );

    let output = cx(&tmp)
        .args(["profiles", "list", "--label", "missing", "--output", "json"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "empty filter should succeed, stderr: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(stdout, "[]", "empty result should be [], got: {stdout}");

    let profiles: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).expect("should be valid JSON");
    assert_eq!(profiles.len(), 0, "should have 0 profiles");
}

#[test]
fn list_json_no_profiles_prints_empty_array() {
    let tmp = temp_home();
    let output = cx(&tmp)
        .args(["profiles", "list", "--output", "json"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(stdout, "[]", "no profiles should print [], got: {stdout}");
}

#[test]
fn list_json_global_flag_before_profiles() {
    let tmp = temp_home();
    write_profile(
        &tmp,
        "demo",
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"k\"\nregion = \"eu1\"\n",
    );

    let output = cx(&tmp)
        .args(["--output", "json", "profiles", "list"])
        .output()
        .expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "should succeed, stderr: {stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let profiles: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).expect("should be valid JSON");
    assert_eq!(profiles.len(), 1, "should have 1 profile");
    assert_eq!(profiles[0]["name"], "demo");
}
