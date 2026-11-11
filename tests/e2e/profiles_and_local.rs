//! Local commands (`profiles`, `init`, `completions`, `cleanup`, `skills`,
//! `schema`) plus the public-docs commands (`docs search` / `docs fetch`).
//!
//! Every test that touches config runs the binary against an isolated temp
//! HOME (`HOME` + `CX_HOME`), so nothing here ever reads or writes `~/.cx`.
//!
//! Tests that need no network are *not* `#[ignore]`d: they run in the normal
//! `cargo test` pass. `init` (verifies credentials against the API) and
//! `docs` (fetches coralogix.com) stay `#[ignore]`d like the rest of the e2e
//! suite.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

use assert_cmd::Command;
use serde_json::Value;

use crate::harness;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh, empty directory to use as `$HOME` for one test.
pub(crate) fn temp_home(tag: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cx_e2e_{tag}_{pid}_{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A `cx` invocation sealed into `home`: no developer profile, no inherited
/// credentials, no `CX_PROFILE`.
pub(crate) fn cx_in_home(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("HOME", home)
        .env("CX_HOME", home)
        .env_remove("CX_API_KEY")
        .env_remove("CX_REGION")
        .env_remove("CX_PROFILE")
        .current_dir(home);
    cmd
}

/// Like [`cx_in_home`] but keeps the credentials resolved by
/// `harness::require_creds` (env-only mode: no profile on disk).
pub(crate) fn cx_in_home_with_creds(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("HOME", home)
        .env("CX_HOME", home)
        .env_remove("CX_PROFILE")
        .current_dir(home);
    cmd
}

fn run(cmd: &mut Command) -> (bool, String, String) {
    let output = cmd.output().expect("failed to run cx");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn profile_path(home: &Path, name: &str) -> PathBuf {
    home.join(".cx")
        .join("profiles")
        .join(format!("{name}.toml"))
}

fn config_toml(home: &Path) -> String {
    fs::read_to_string(home.join(".cx").join("config.toml")).unwrap_or_default()
}

// ── profiles ─────────────────────────────────────────────────────────────────

/// add -> list -> add (set-default) -> set-default -> refresh -> delete, all
/// non-interactively inside a temp HOME.
#[test]
fn profiles_lifecycle_in_temp_home() {
    let home = temp_home("profiles");

    let (ok, _, err) = run(cx_in_home(&home).args([
        "profiles",
        "add",
        "alpha",
        "--api-key",
        "fake-key-alpha",
        "--region",
        "eu2",
    ]));
    assert!(ok, "profiles add alpha failed: {err}");
    assert!(profile_path(&home, "alpha").is_file());

    // The first profile becomes the default on its own.
    let (ok, out, _) = run(cx_in_home(&home).args(["profiles", "list"]));
    assert!(ok);
    let alpha_line = out
        .lines()
        .find(|l| l.starts_with("alpha"))
        .unwrap_or_else(|| panic!("alpha missing from list:\n{out}"));
    assert!(alpha_line.contains("eu2") && alpha_line.contains("api-key"));
    assert!(
        alpha_line.ends_with("yes"),
        "alpha should be default: {alpha_line}"
    );

    // The --name flag and --set-default on a second profile.
    let (ok, _, err) = run(cx_in_home(&home).args([
        "profiles",
        "add",
        "--name",
        "beta",
        "--api-key",
        "fake-key-beta",
        "--region",
        "us1",
        "--set-default",
    ]));
    assert!(ok, "profiles add beta failed: {err}");
    assert!(config_toml(&home).contains("default_profile = \"beta\""));

    // Re-adding without --force is refused non-interactively; --force overwrites.
    let (ok, _, err) = run(cx_in_home(&home).args([
        "profiles",
        "add",
        "beta",
        "--api-key",
        "fake-key-beta-2",
        "--region",
        "us1",
    ]));
    assert!(!ok);
    assert!(err.contains("already exists"), "stderr: {err}");
    let (ok, _, err) = run(cx_in_home(&home).args([
        "profiles",
        "add",
        "beta",
        "--api-key",
        "fake-key-beta-2",
        "--region",
        "us2",
        "--force",
    ]));
    assert!(ok, "profiles add --force failed: {err}");
    assert!(fs::read_to_string(profile_path(&home, "beta"))
        .unwrap()
        .contains("us2"));

    let (ok, out, _) = run(cx_in_home(&home).args(["profiles", "set-default", "alpha"]));
    assert!(ok);
    assert!(
        out.contains("Default profile set to 'alpha'"),
        "stdout: {out}"
    );
    assert!(config_toml(&home).contains("default_profile = \"alpha\""));

    // refresh is OAuth-only: an API-key profile is refused, a missing one is not found.
    let (ok, _, err) = run(cx_in_home(&home).args(["profiles", "refresh", "alpha"]));
    assert!(!ok);
    assert!(err.contains("uses an API key"), "stderr: {err}");
    let (ok, _, err) = run(cx_in_home(&home).args(["profiles", "refresh", "ghost"]));
    assert!(!ok);
    assert!(err.contains("not found"), "stderr: {err}");

    let (ok, out, _) = run(cx_in_home(&home).args(["profiles", "delete", "beta", "--force"]));
    assert!(ok);
    assert!(out.contains("Profile 'beta' deleted"), "stdout: {out}");
    assert!(!profile_path(&home, "beta").exists());
    let (ok, out, _) = run(cx_in_home(&home).args(["profiles", "delete", "alpha", "-f"]));
    assert!(ok);
    assert!(
        out.contains("was your default profile"),
        "deleting the default must say so: {out}"
    );
    let (ok, _, err) = run(cx_in_home(&home).args(["profiles", "delete", "alpha", "--force"]));
    assert!(!ok);
    assert!(err.contains("not found"), "stderr: {err}");

    let (ok, out, _) = run(cx_in_home(&home).args(["profiles", "list"]));
    assert!(ok);
    assert!(out.contains("No profiles configured"), "stdout: {out}");
}

/// `cx profiles list` inherits the developer's config; this only asserts the
/// command runs. The real assertions live in `profiles_lifecycle_in_temp_home`.
#[test]
#[ignore]
fn profiles_list() {
    if harness::require_creds("profiles_list").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["profiles", "list"]);
}

// ── init ─────────────────────────────────────────────────────────────────────

fn e2e_api_key() -> Option<String> {
    harness::require_creds("init")?;
    std::env::var("CX_API_KEY").ok().filter(|k| !k.is_empty())
}

fn e2e_region() -> String {
    std::env::var("CX_REGION").unwrap_or_else(|_| "stg1".to_string())
}

fn assert_init_created_default_profile(home: &Path, out: &str, region: &str) {
    assert!(
        out.contains("Connected"),
        "init should verify the key: {out}"
    );
    assert!(
        profile_path(home, "default").is_file(),
        "init must write profiles/default.toml"
    );
    let profile = fs::read_to_string(profile_path(home, "default")).unwrap();
    assert!(
        profile.contains(&format!("\"{region}\"")),
        "profile: {profile}"
    );
    assert!(config_toml(home).contains("default_profile = \"default\""));
}

/// `cx init --api-key ... --region ... --no-skills` is the fully prompt-free
/// onboarding path. A second run must be idempotent.
#[test]
#[ignore]
fn init_non_interactive_with_api_key_flag() {
    let Some(key) = e2e_api_key() else {
        eprintln!("[e2e] skipping init_non_interactive_with_api_key_flag: CX_API_KEY not in env");
        return;
    };
    let region = e2e_region();
    let home = temp_home("init_flag");

    let (ok, out, err) = run(cx_in_home(&home).args([
        "init",
        "--api-key",
        &key,
        "--region",
        &region,
        "--no-skills",
    ]));
    assert!(ok, "init failed: {err}");
    assert_init_created_default_profile(&home, &out, &region);

    // Idempotent: the profile step is skipped, the exit code stays 0.
    let (ok, out, err) = run(cx_in_home(&home).args(["init", "--no-skills"]));
    assert!(ok, "second init failed: {err}");
    assert!(
        out.contains("already configured"),
        "second init must skip the profile step: {out}"
    );
}

/// Same path with the key coming from `CX_API_KEY` instead of the flag.
#[test]
#[ignore]
fn init_non_interactive_with_env_api_key() {
    let Some(key) = e2e_api_key() else {
        eprintln!("[e2e] skipping init_non_interactive_with_env_api_key: CX_API_KEY not in env");
        return;
    };
    let region = e2e_region();
    let home = temp_home("init_env");
    let (ok, out, err) = run(cx_in_home(&home).env("CX_API_KEY", &key).args([
        "init",
        "--region",
        &region,
        "--no-skills",
    ]));
    assert!(ok, "init failed: {err}");
    assert_init_created_default_profile(&home, &out, &region);
    assert!(
        fs::read_to_string(profile_path(&home, "default"))
            .unwrap()
            .contains(&key),
        "the env key must be stored in the profile file"
    );
}

// ── completions ──────────────────────────────────────────────────────────────

#[test]
fn completions_generate_prints_a_script_for_each_shell() {
    for shell in ["zsh", "bash", "fish", "powershell"] {
        let (ok, out, err) =
            run(cx_in_home(&temp_home("gen")).args(["completions", "generate", shell]));
        assert!(ok, "completions generate {shell} failed: {err}");
        assert!(
            out.contains("cx") && out.contains("COMPLETE"),
            "{shell} script should register the dynamic COMPLETE hook for cx: {out}"
        );
    }
    let (ok, _, err) =
        run(cx_in_home(&temp_home("gen")).args(["completions", "generate", "elvish"]));
    assert!(!ok);
    assert!(err.contains("not supported"), "stderr: {err}");
}

#[test]
fn completions_install_then_refresh_in_temp_home() {
    let home = temp_home("completions");

    let (ok, _, err) = run(cx_in_home(&home).args(["completions", "refresh"]));
    assert!(
        ok,
        "refresh with nothing installed must still exit 0: {err}"
    );
    assert!(
        err.contains("No managed completions found"),
        "stderr: {err}"
    );

    // Default path for zsh lives under $HOME, so it lands in the temp HOME.
    let (ok, _, err) = run(cx_in_home(&home).args(["completions", "install", "zsh"]));
    assert!(ok, "install zsh failed: {err}");
    let zsh_path = home.join(".zfunc").join("_cx");
    assert!(zsh_path.is_file(), "expected {}", zsh_path.display());
    assert!(err.contains("Installed zsh completions"), "stderr: {err}");
    let config = config_toml(&home);
    assert!(config.contains("[[managed_completions]]") && config.contains("shell = \"zsh\""));

    // --path overrides the default location and is tracked too.
    let bash_path = home.join("custom").join("cx.bash");
    let (ok, _, err) = run(cx_in_home(&home).args([
        "completions",
        "install",
        "bash",
        "--path",
        bash_path.to_str().unwrap(),
    ]));
    assert!(ok, "install bash --path failed: {err}");
    assert!(bash_path.is_file());

    // PowerShell has no default path: the error names --path.
    let (ok, _, err) = run(cx_in_home(&home).args(["completions", "install", "powershell"]));
    assert!(!ok);
    assert!(err.contains("--path"), "stderr: {err}");

    // Refresh rewrites every tracked script.
    fs::write(&zsh_path, "stale").unwrap();
    let (ok, _, err) = run(cx_in_home(&home).args(["completions", "refresh"]));
    assert!(ok, "refresh failed: {err}");
    assert!(
        err.contains("Refreshed zsh completions") && err.contains("Refreshed bash completions")
    );
    assert_ne!(fs::read_to_string(&zsh_path).unwrap(), "stale");
}

// ── cleanup ──────────────────────────────────────────────────────────────────

/// `cleanup` removes `cx_results*` files older than 30 minutes from the
/// configured `temp_dir` and leaves fresh ones alone.
#[test]
fn cleanup_removes_only_stale_result_files() {
    let home = temp_home("cleanup");
    let spill_dir = home.join("spill");
    fs::create_dir_all(&spill_dir).unwrap();
    fs::create_dir_all(home.join(".cx")).unwrap();
    fs::write(
        home.join(".cx").join("config.toml"),
        format!("temp_dir = \"{}/\"\n", spill_dir.display()),
    )
    .unwrap();

    let stale = spill_dir.join("cx_results_stale.json");
    let fresh = spill_dir.join("cx_results_fresh.json");
    let unrelated = spill_dir.join("other_stale.json");
    for path in [&stale, &fresh, &unrelated] {
        fs::write(path, "[]").unwrap();
    }
    let two_hours_ago = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
    for path in [&stale, &unrelated] {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();
    }

    let (ok, out, err) = run(cx_in_home(&home).arg("cleanup"));
    assert!(ok, "cleanup failed: {err}");
    assert!(out.contains("Removed 1 stale result file"), "stdout: {out}");
    assert!(!stale.exists(), "stale cx_results file must be removed");
    assert!(fresh.exists(), "fresh cx_results file must be kept");
    assert!(
        unrelated.exists(),
        "files without the cx_results prefix must be kept"
    );

    let (ok, out, _) = run(cx_in_home(&home).arg("cleanup"));
    assert!(ok);
    assert!(out.contains("No stale result files found"), "stdout: {out}");
}

/// `cx cleanup` against the developer's own config only has to run.
#[test]
#[ignore]
fn cleanup() {
    if harness::require_creds("cleanup").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["cleanup"]);
}

// ── skills ───────────────────────────────────────────────────────────────────

/// The installer needs `npx`, so with an empty PATH the only acceptable
/// outcome is the actionable Node.js error (nothing is downloaded or written).
/// `tests/skills` drives the full install through a fake `npx`; this is the
/// e2e suite's tripwire that the real binary still fails helpfully.
#[test]
fn skills_install_without_npx_reports_helpful_error() {
    let home = temp_home("skills");
    let empty_path = temp_home("skills_empty_path");
    let (ok, _, err) = run(cx_in_home(&home)
        .env("PATH", &empty_path)
        .args(["skills", "install", "--global"]));
    assert!(!ok, "skills install must fail without npx");
    assert!(
        err.contains("Node.js") && err.contains("npx"),
        "expected the actionable Node.js/npx error, stderr: {err}"
    );
    assert!(
        !home.join(".agents").exists() && !home.join(".claude").exists(),
        "nothing may be installed when npx is missing"
    );
}

// ── schema ───────────────────────────────────────────────────────────────────

fn find_command<'a>(commands: &'a [Value], name: &str) -> &'a Value {
    commands
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("schema has no command '{name}'"))
}

fn argument_names(cmd: &Value) -> Vec<String> {
    cmd["arguments"]
        .as_array()
        .map(|args| {
            args.iter()
                .filter_map(|a| a["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// `cx schema` is how agents discover flags, so every flag this suite drives
/// must be listed there with its `--flag` spelling. `tests/schema` checks the
/// command set; this checks the flag surface of the query commands.
#[test]
fn schema_lists_the_flags_of_the_query_commands() {
    let (ok, out, err) = run(cx_in_home(&temp_home("schema")).args(["schema", "-o", "json"]));
    assert!(ok, "cx schema failed: {err}");
    let schema: Value = serde_json::from_str(&out).expect("schema must be valid JSON");
    assert!(schema["version"].is_string());
    let commands = schema["commands"].as_array().expect("commands array");

    let logs = find_command(commands, "logs");
    let names = argument_names(logs);
    for flag in [
        "query",
        "start",
        "end",
        "limit",
        "tier",
        "strict_fields_validation",
        "no_strict_fields_validation",
        "extended_information",
    ] {
        assert!(
            names.contains(&flag.to_string()),
            "logs is missing '{flag}': {names:?}"
        );
    }
    let tier = logs["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "tier")
        .unwrap();
    assert_eq!(tier["flag"], "--tier");
    let strict = logs["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "strict_fields_validation")
        .unwrap();
    assert_eq!(strict["type"], "boolean");

    let spans = argument_names(find_command(commands, "spans"));
    assert!(spans.contains(&"extended_information".to_string()));

    let search_fields = argument_names(find_command(commands, "search-fields"));
    for flag in ["text", "search_type", "dataset", "limit", "offset"] {
        assert!(
            search_fields.contains(&flag.to_string()),
            "search-fields: {search_fields:?}"
        );
    }

    let olly = find_command(commands, "olly");
    let ask = find_command(olly["subcommands"].as_array().unwrap(), "ask");
    let ask_args = argument_names(ask);
    for flag in [
        "message",
        "chat_id",
        "model",
        "timeout",
        "agent_to_agent_mode",
    ] {
        assert!(
            ask_args.contains(&flag.to_string()),
            "olly ask: {ask_args:?}"
        );
    }

    let dataprime = find_command(commands, "dataprime");
    let subs: Vec<&str> = dataprime["subcommands"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(subs, ["list", "show", "query"]);
}

// ── docs (public documentation, no credentials) ──────────────────────────────

#[test]
#[ignore]
fn docs_search_then_fetch_first_hit() {
    let v = harness::run_ok_json(&["docs", "search", "alerts", "--limit", "3", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["rank", "title", "suffix"]);
    let hits = v.as_array().unwrap();
    assert!(
        hits.len() <= 3,
        "--limit 3 must cap the hits: {}",
        hits.len()
    );
    assert_eq!(hits[0]["rank"], 1);
    assert!(
        hits.iter().any(|h| h["title"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase()
            .contains("alert")),
        "a search for 'alerts' should rank an alerts page: {v}"
    );

    let suffix = hits[0]["suffix"].as_str().unwrap().to_string();
    let page = harness::run_ok_json(&["docs", "fetch", &suffix, "-o", "json"]);
    harness::assert_object_with_keys(&page, &["suffix", "body"]);
    assert!(
        !page["body"].as_str().unwrap_or_default().trim().is_empty(),
        "fetched page must have a body"
    );

    let text = harness::run_ok_nonempty(&["docs", "search", "alerts", "--limit", "2"]);
    let text = String::from_utf8_lossy(&text);
    assert!(
        text.starts_with("1. "),
        "text search is a ranked list: {text}"
    );
}

#[test]
#[ignore]
fn docs_fetch_known_page_as_text_and_toon() {
    let suffix = "user-guides/data_exploration/spans.md";
    let text = harness::run_ok_nonempty(&["docs", "fetch", suffix]);
    let text = String::from_utf8_lossy(&text);
    assert!(
        text.starts_with("Path: user-guides/data_exploration/spans"),
        "text fetch prints the path first: {text}"
    );
    assert!(
        text.contains("# Explore spans"),
        "expected the page heading: {text}"
    );

    let toon = harness::run_ok_nonempty(&["docs", "fetch", suffix, "-o", "toon"]);
    assert!(String::from_utf8_lossy(&toon).contains("Explore spans"));

    // A full URL is not a suffix.
    let output = harness::cx()
        .args(["docs", "fetch", "https://coralogix.com/docs/x"])
        .output()
        .expect("cx must run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("path suffix"));
}
