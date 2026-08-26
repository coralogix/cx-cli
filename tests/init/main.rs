//! Integration tests for `cx init` (FORGE-658).
//!
//! `cx init` chains `cx profiles add`, an authenticated health check
//! (`GET /identity/whoami`, FORGE-660), a best-effort onboarding recording for
//! OAuth profiles (`POST /api/v2/onboarding`, FORGE-876), and `cx skills
//! install`. These tests
//! drive the non-interactive (advanced) path — the coding-agent one-liner — so
//! no terminal or Node.js is required: the skills installer is a fake `npx` on
//! PATH that records its arguments, exactly as the skills tests do.
//!
//! Tests point `--url` at a `wiremock` server. Its `127.0.0.1` address is not a
//! known Coralogix region, so `cx init` treats it as a custom endpoint and
//! skips the health check. Most tests therefore exercise the profile/skills
//! chain; the two `*_custom_endpoint*` tests assert the probe is skipped (the
//! identity route receives zero requests).

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cx_test_init_{tag}_{pid}_{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Start a mock server purely to supply a reachable `--url`. Its `127.0.0.1`
/// address is not a known Coralogix region, so `cx init` classifies it as a
/// custom endpoint and skips the credential check — the identity route is never
/// called. Returns the server (keep it alive) so `server.uri()` can be passed
/// as `--url`.
async fn custom_endpoint_server() -> MockServer {
    MockServer::start().await
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

// ── Advanced (non-interactive) profile + verify + skills chain ─────────────────

#[cfg(unix)]
#[tokio::test]
async fn init_writes_profile_then_installs_skills() {
    let server = custom_endpoint_server().await;
    let home = temp_dir("chain");
    let bin = temp_dir("chain_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file);

    cx(&home, &bin)
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "faketoken",
            "--global-skills",
        ])
        .assert()
        .success();

    // Step 1: profile written with the quick-setup defaults (file + json).
    let toml = fs::read_to_string(profile_path(&home, "default")).unwrap();
    assert!(toml.contains("auth = \"api_key\""), "profile: {toml}");
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

/// A custom / BYOC endpoint (any URL that doesn't resolve to a known Coralogix
/// region) skips the health check entirely: such deployments may not expose
/// `GET /identity/whoami`, so probing it would be a false negative. Since the
/// wiremock URL is itself a custom endpoint, we assert the route is never hit,
/// init still succeeds, and the user is told the check was skipped.
#[cfg(unix)]
#[tokio::test]
async fn init_skips_verification_for_custom_endpoint() {
    let server = MockServer::start().await;
    // Mounted so any stray call is answerable, but it must receive zero hits.
    Mock::given(method("GET"))
        .and(path("/identity/whoami"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "team_id": 53623,
            "user_name": "agent@example.com"
        })))
        .expect(0)
        .mount(&server)
        .await;

    let home = temp_dir("verify");
    let bin = temp_dir("verify_bin");
    install_fake_npx(&bin, &bin.join("args.txt"));

    let output = cx(&home, &bin)
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "faketoken",
            "--global-skills",
        ])
        .output()
        .expect("failed to run cx");

    assert!(
        output.status.success(),
        "init should succeed for a custom endpoint (verification skipped)"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Skipped the automatic credential check"),
        "expected the skip notice for a custom endpoint, stdout: {stdout}"
    );
    // No identity probe was issued against the custom endpoint.
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        0,
        "the whoami health check must not run for a custom endpoint"
    );
}

/// The bot's scenario (FORGE-660 review): a custom endpoint whose identity
/// route would error must NOT block onboarding. Because the probe is skipped
/// before any call, even a 401-returning identity route never runs — init
/// succeeds, writes the profile, and proceeds to the skills step.
#[cfg(unix)]
#[tokio::test]
async fn init_custom_endpoint_does_not_block_on_identity_route() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/identity/whoami"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(serde_json::json!({ "message": "bad key" })),
        )
        .expect(0)
        .mount(&server)
        .await;

    let home = temp_dir("verifyfail");
    let bin = temp_dir("verifyfail_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file);

    let output = cx(&home, &bin)
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "faketoken",
            "--global-skills",
        ])
        .output()
        .expect("failed to run cx");

    assert!(
        output.status.success(),
        "a custom endpoint must not fail onboarding on the identity route"
    );
    // The profile is written and the skills step still runs.
    assert!(
        profile_path(&home, "default").exists(),
        "profile must be written for a custom endpoint"
    );
    assert!(
        args_file.exists(),
        "the skills installer must run after a skipped verification"
    );
    // The identity route was never probed.
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        0,
        "the whoami health check must not run for a custom endpoint"
    );
}

/// The onboarding recording (`POST /api/v2/onboarding`, FORGE-876) must never
/// fire on the standard init path exercised here: these tests run against a
/// custom (non-region) endpoint, and the call is gated to known regions
/// regardless of auth kind. So the route must receive zero hits, and init must
/// still succeed. (The POST's own behavior is unit-tested in
/// `src/commands/init/api.rs`; a true-positive here would need a known-region
/// host resolving to a local mock, which the harness cannot fake.)
#[cfg(unix)]
#[tokio::test]
async fn init_does_not_report_onboarding_for_custom_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/onboarding"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;

    let home = temp_dir("noonboard");
    let bin = temp_dir("noonboard_bin");
    install_fake_npx(&bin, &bin.join("args.txt"));

    cx(&home, &bin)
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "faketoken",
            "--global-skills",
        ])
        .assert()
        .success();

    assert_eq!(
        server.received_requests().await.unwrap().len(),
        0,
        "the onboarding POST must not run for a custom-endpoint init"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn init_agents_flag_reaches_the_installer() {
    let server = custom_endpoint_server().await;
    let home = temp_dir("agents");
    let bin = temp_dir("agents_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file);

    cx(&home, &bin)
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "k",
            "--global-skills",
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
#[tokio::test]
async fn init_no_skills_skips_the_installer() {
    let server = custom_endpoint_server().await;
    let home = temp_dir("noskills");
    let bin = temp_dir("noskills_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file);

    cx(&home, &bin)
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "k",
            "--no-skills",
        ])
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

/// A non-interactive run without `--global-skills`/`--local-skills` cannot resolve a scope,
/// so the skills step fails inside the skills command — but init downgrades
/// that to a warning and still succeeds, having written the profile.
#[cfg(unix)]
#[tokio::test]
async fn init_without_scope_warns_but_succeeds() {
    let server = custom_endpoint_server().await;
    let home = temp_dir("noscope");
    let bin = temp_dir("noscope_bin");
    install_fake_npx(&bin, &bin.join("args.txt"));

    let output = cx(&home, &bin)
        .args(["init", "--url", &server.uri(), "--api-key", "k"])
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
#[tokio::test]
async fn init_without_npx_warns_but_succeeds() {
    let server = custom_endpoint_server().await;
    let home = temp_dir("nonpx");
    let empty_path = temp_dir("nonpx_path"); // no npx on PATH

    let output = cx(&home, &empty_path)
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "k",
            "--global-skills",
        ])
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
#[tokio::test]
async fn init_skips_profile_setup_when_one_already_exists() {
    let server = custom_endpoint_server().await;
    let home = temp_dir("idempotent");
    let bin = temp_dir("idempotent_bin");
    let args_file = bin.join("args.txt");
    install_fake_npx(&bin, &args_file);

    // First run creates the default profile pointing at the mock endpoint.
    cx(&home, &bin)
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "first",
            "--global-skills",
        ])
        .assert()
        .success();
    let _ = fs::remove_file(&args_file); // so we can prove skills ran the 2nd time

    // Second run supplies different credentials, but the existing profile must
    // win: init skips profile setup entirely.
    let output = cx(&home, &bin)
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "second",
            "--global-skills",
        ])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("already configured") && stdout.contains("skipping"),
        "expected a skip notice on the second run, stdout: {stdout}"
    );

    // The profile is unchanged — still the first key.
    let toml = fs::read_to_string(profile_path(&home, "default")).unwrap();
    assert!(
        toml.contains("first"),
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
#[tokio::test]
async fn init_skips_skills_when_already_installed() {
    use std::os::unix::fs::PermissionsExt;
    let server = custom_endpoint_server().await;
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
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "k",
            "--global-skills",
        ])
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
