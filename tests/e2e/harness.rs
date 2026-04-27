//! Shared harness for cx e2e tests.
//!
//! Resolves credentials and builds `cx` subprocess invocations via
//! `assert_cmd`. Per-domain ID discovery (e.g. picking a real alert id to
//! pass to `alerts get`) lives in each test module — see
//! `tests/e2e/alerts.rs` for the pattern.
//!
//! All e2e tests are gated by `#[ignore]` and additionally skip with a clear
//! `[e2e]` log line when no credentials are available, so the suite is safe
//! to run on a developer machine that hasn't been configured for staging.

use std::sync::OnceLock;

use assert_cmd::Command;
use serde_json::Value;

pub const SHORT_WINDOW_START: &str = "now-15m";
pub const SMALL_LIMIT: &str = "10";

/// Returns `Some(())` if Coralogix credentials are available (env or
/// `.env` file), and exports `CX_API_KEY`/`CX_REGION` into the test
/// process so the `cx` subprocess inherits them. Returns `None` (after
/// printing a skip message) if no key was found.
pub fn require_creds(test_name: &str) -> Option<()> {
    static INIT: OnceLock<bool> = OnceLock::new();
    let ok = *INIT.get_or_init(|| {
        if std::env::var("CX_API_KEY").is_ok() {
            ensure_region();
            return true;
        }
        let _ = dotenvy::dotenv();
        if std::env::var("CX_API_KEY").is_ok() {
            ensure_region();
            return true;
        }
        false
    });
    if !ok {
        eprintln!(
            "[e2e] skipping {test_name}: no CX_API_KEY in env or .env (run with CX_API_KEY=... CX_REGION=stg1)"
        );
        return None;
    }
    Some(())
}

fn ensure_region() {
    if std::env::var("CX_REGION").is_err() {
        std::env::set_var("CX_REGION", "stg1");
    }
}

/// Build a `cx` invocation. Inherits the parent process env, so credentials
/// resolved by `require_creds` flow through automatically.
pub fn cx() -> Command {
    Command::cargo_bin("cx").expect("cx binary should build")
}

/// Run `cx <args>` and assert success. Returns captured stdout bytes.
///
/// Captured stdout/stderr are echoed via `println!`/`eprintln!` so they
/// surface when the test runner is invoked with `--nocapture`. Without
/// that flag they're hidden on success and shown on failure, which is
/// what cargo's normal capture behaviour does anyway.
pub fn run_ok(args: &[&str]) -> Vec<u8> {
    let assert = cx().args(args).assert().success();
    let output = assert.get_output();
    let stdout = output.stdout.clone();

    println!("\n$ cx {}", args.join(" "));
    if !stdout.is_empty() {
        println!("--- stdout ---");
        println!("{}", String::from_utf8_lossy(&stdout));
    }
    if !output.stderr.is_empty() {
        println!("--- stderr ---");
        println!("{}", String::from_utf8_lossy(&output.stderr));
    }

    stdout
}

/// Run `cx <args>`, assert success and that stdout is non-empty. Returns
/// stdout bytes. Use this for commands whose contract is "produce some
/// human-readable output" (e.g. text/agents modes, local-only commands).
pub fn run_ok_nonempty(args: &[&str]) -> Vec<u8> {
    let stdout = run_ok(args);
    assert!(
        !stdout.is_empty(),
        "expected non-empty stdout from `cx {}`",
        args.join(" ")
    );
    stdout
}

/// Run `cx <args> -o json`-style command, assert success, and parse stdout
/// as JSON. Empty arrays/objects are valid — we only fail on malformed
/// payloads or empty stdout. Returns the parsed value.
pub fn run_ok_json(args: &[&str]) -> Value {
    let stdout = run_ok_nonempty(args);
    serde_json::from_slice(&stdout).unwrap_or_else(|e| {
        panic!(
            "expected valid JSON on stdout from `cx {}`: {e}\nstdout: {}",
            args.join(" "),
            String::from_utf8_lossy(&stdout)
        )
    })
}

/// Parse stdout bytes as JSON. Returns `None` if not valid JSON — used by
/// discovery helpers that treat malformed payloads as a skip condition.
pub fn parse_json(stdout: &[u8]) -> Option<Value> {
    serde_json::from_slice(stdout).ok()
}

// ── Shape assertions ─────────────────────────────────────────────────
//
// These check the *structure* of a JSON response without inspecting values.
// Empty arrays pass vacuously — that's intentional, since staging may
// genuinely have no data — but they catch field renames, type changes
// (array → object, string → number), and missing keys whenever data is
// present.

/// Asserts `v` is a JSON array. Returns a reference to the elements.
pub fn assert_array(v: &Value) -> &[Value] {
    v.as_array()
        .unwrap_or_else(|| panic!("expected JSON array, got: {v}"))
}

/// Asserts `v` is an array, and every element is an object containing
/// every key in `required_keys` (values may be any type, including null).
/// Empty arrays pass vacuously.
pub fn assert_array_of_objects_with_keys(v: &Value, required_keys: &[&str]) {
    let arr = assert_array(v);
    for (i, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .unwrap_or_else(|| panic!("element {i} is not an object: {item}"));
        for key in required_keys {
            assert!(
                obj.contains_key(*key),
                "element {i} missing key '{key}': {item}"
            );
        }
    }
}

/// Asserts `v` is an array of JSON strings. Empty arrays pass vacuously.
pub fn assert_array_of_strings(v: &Value) {
    let arr = assert_array(v);
    for (i, item) in arr.iter().enumerate() {
        assert!(item.is_string(), "element {i} is not a string: {item}");
    }
}

/// Asserts `v` is an object containing every key in `required_keys`.
pub fn assert_object_with_keys(v: &Value, required_keys: &[&str]) {
    let obj = v
        .as_object()
        .unwrap_or_else(|| panic!("expected JSON object, got: {v}"));
    for key in required_keys {
        assert!(obj.contains_key(*key), "object missing key '{key}': {v}");
    }
}
