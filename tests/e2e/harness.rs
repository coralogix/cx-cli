//! Shared harness for cx e2e tests.
//!
//! Resolves credentials and builds `cx` subprocess invocations via
//! `assert_cmd`. Per-domain ID discovery (e.g. picking a real alert id to
//! pass to `alerts get`) lives in each test module - see
//! `tests/e2e/alerts.rs` for the pattern.
//!
//! All e2e tests are gated by `#[ignore]` and additionally skip with a clear
//! `[e2e]` log line when no credentials are available, so the suite is safe
//! to run on a developer machine without test team credentials.

use std::sync::OnceLock;

use assert_cmd::Command;
use serde_json::Value;

pub const SHORT_WINDOW_START: &str = "now-15m";
pub const SMALL_LIMIT: &str = "10";

/// Returns `Some(())` if Coralogix credentials are available (env,
/// `.env` file, or a `~/.cx` profile), and exports `CX_API_KEY`/`CX_REGION`
/// into the test process so the `cx` subprocess inherits them. Returns
/// `None` (after printing a skip message) if no credentials were found.
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
        if has_default_profile() {
            return true;
        }
        false
    });
    if !ok {
        eprintln!(
            "[e2e] skipping {test_name}: no CX_API_KEY in env or .env and no ~/.cx profile found"
        );
        return None;
    }
    Some(())
}

fn has_default_profile() -> bool {
    dirs::home_dir()
        .map(|h| {
            h.join(".cx")
                .join("profiles")
                .join("default.toml")
                .is_file()
        })
        .unwrap_or(false)
}

fn ensure_region() {
    if std::env::var("CX_REGION").is_err() {
        std::env::set_var("CX_REGION", "eu2");
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
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("\n$ cx {}", args.join(" "));
    if !stdout.is_empty() {
        println!("--- stdout ---");
        println!("{}", String::from_utf8_lossy(&stdout));
    }
    if !stderr.is_empty() {
        println!("--- stderr ---");
        println!("{stderr}");
    }

    assert!(
        !stderr.contains("API request failed"),
        "cx {} returned API errors on stderr:\n{stderr}",
        args.join(" ")
    );

    assert!(
        !stderr.contains("Authentication failed"),
        "cx {} returned auth errors on stderr (expired token or insufficient key permissions):\n{stderr}",
        args.join(" ")
    );

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
/// as JSON. Empty arrays/objects are valid - we only fail on malformed
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

/// Parse stdout bytes as JSON. Returns `None` if not valid JSON - used by
/// discovery helpers that treat malformed payloads as a skip condition.
pub fn parse_json(stdout: &[u8]) -> Option<Value> {
    serde_json::from_slice(stdout).ok()
}

// ── Shape assertions ─────────────────────────────────────────────────
//
// These check the *structure* of a JSON response without inspecting values.
// Empty arrays pass vacuously - that's intentional, since the test team
// may genuinely have no data - but they catch field renames, type changes
// (array → object, string → number), and missing keys whenever data is
// present.

/// Asserts `v` is a JSON array. Returns a reference to the elements.
pub fn assert_array(v: &Value) -> &[Value] {
    v.as_array()
        .unwrap_or_else(|| panic!("expected JSON array, got: {v}"))
}

/// Like `assert_array_of_objects_with_keys` but also asserts the array is
/// non-empty. Use this for list commands where the test team is expected to
/// always have at least one resource.
pub fn assert_nonempty_array_of_objects_with_keys(v: &Value, required_keys: &[&str]) {
    let arr = assert_array(v);
    assert!(!arr.is_empty(), "expected non-empty array, got empty []");
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

/// For `get` responses that may be wrapped (e.g. `{"action": {...}}`),
/// unwrap the first single-key object or return as-is if it already has
/// the expected keys. Then assert required keys.
pub fn assert_get_response(v: &Value, required_keys: &[&str]) {
    let obj = v
        .as_object()
        .unwrap_or_else(|| panic!("expected JSON object, got: {v}"));

    let has_keys = required_keys.iter().all(|k| obj.contains_key(*k));
    if has_keys {
        return;
    }

    if obj.len() == 1 {
        if let Some(inner) = obj.values().next() {
            if let Some(inner_obj) = inner.as_object() {
                for key in required_keys {
                    assert!(
                        inner_obj.contains_key(*key),
                        "inner object missing key '{key}': {v}"
                    );
                }
                return;
            }
        }
    }

    panic!("get response missing required keys {required_keys:?}: {v}");
}

// ── Write-capable e2e support ───────────────────────────────────────────────
//
// Every mutating e2e test runs against a dedicated writable test team. PR
// runs can overlap, so nothing may depend on shared state: each test
// creates its own resources under a run-scoped unique name, asserts on
// exactly those, and deletes them on the way out (also on panic, via
// [`Cleanup`]). A sweeper (`tests/e2e/sweeper.rs`) removes leftovers from
// crashed runs by parsing the timestamp embedded in the name.
//
// Name format: `cx-e2e-<kind>-<YYYYmmddHHMM>-<run-id>-<seq>`. Only
// `[A-Za-z0-9-]` so the name is safe for every API that validates names.

use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub const E2E_NAME_PREFIX: &str = "cx-e2e-";

/// Writes are enabled when `CX_E2E_ALLOW_WRITES` is truthy. CI sets it;
/// developers opt in explicitly so a stray `cargo test --test e2e` with
/// personal credentials never mutates a real team.
pub fn writes_allowed() -> bool {
    std::env::var("CX_E2E_ALLOW_WRITES")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

/// Like [`require_creds`], but also requires write mode. Prints a skip line
/// and returns `None` otherwise.
pub fn require_write_creds(test_name: &str) -> Option<()> {
    require_creds(test_name)?;
    if !writes_allowed() {
        eprintln!("[e2e] skipping {test_name}: set CX_E2E_ALLOW_WRITES=1 to run write tests");
        return None;
    }
    Some(())
}

/// Identifier shared by every test in one process. `CX_E2E_RUN_ID` wins
/// (CI sets it to the workflow run id + attempt), else a random id.
pub fn run_id() -> &'static str {
    static RUN_ID: OnceLock<String> = OnceLock::new();
    RUN_ID.get_or_init(|| {
        // Lowercase: several APIs (TCO application rules, parsing-rule
        // matchers) lowercase names on write, and tests compare by name.
        if let Ok(id) = std::env::var("CX_E2E_RUN_ID") {
            let id: String = id
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_lowercase())
                .take(16)
                .collect();
            if !id.is_empty() {
                return id;
            }
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{:x}{:x}", std::process::id(), nanos)
    })
}

/// Build a unique, run-scoped resource name. `kind` is a short slug such as
/// `view` or `alert`. See the module comment for the format.
pub fn unique_name(kind: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let stamp = chrono::Utc::now().format("%Y%m%d%H%M");
    format!("{E2E_NAME_PREFIX}{kind}-{stamp}-{}-{seq}", run_id())
}

/// Parse the `YYYYmmddHHMM` stamp out of a name produced by [`unique_name`].
/// Returns `None` for names that are not ours.
pub fn stamp_from_name(name: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let rest = name.strip_prefix(E2E_NAME_PREFIX)?;
    let stamp = rest
        .split('-')
        .find(|part| part.len() == 12 && part.chars().all(|c| c.is_ascii_digit()))?;
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%d%H%M")
        .ok()
        .map(|dt| dt.and_utc())
}

/// True when `name` was produced by [`unique_name`] in a *different* run
/// and is older than `max_age`. Used by the sweeper.
pub fn is_stale_e2e_name(name: &str, max_age: Duration) -> bool {
    if !name.starts_with(E2E_NAME_PREFIX) || name.contains(run_id()) {
        return false;
    }
    match stamp_from_name(name) {
        Some(stamp) => {
            chrono::Utc::now() - stamp > chrono::Duration::from_std(max_age).unwrap_or_default()
        }
        // A `cx-e2e-` name without a stamp is a probe/leftover; sweep it.
        None => true,
    }
}

/// A JSON definition written to a temp file, removed on drop. Pass
/// `.path()` to `--from-file`.
pub struct JsonFile {
    path: std::path::PathBuf,
}

impl JsonFile {
    pub fn new(label: &str, body: &Value) -> Self {
        let path = std::env::temp_dir().join(format!("{}.json", unique_name(label)));
        std::fs::write(&path, serde_json::to_vec_pretty(body).unwrap()).expect("write temp json");
        Self { path }
    }

    pub fn path(&self) -> &str {
        self.path.to_str().expect("utf-8 temp path")
    }
}

impl Drop for JsonFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Runs `cx <args> --yes` when dropped, ignoring the result. Register it
/// right after a successful create so the resource is removed even if a
/// later assertion panics. Call [`Cleanup::disarm`] once the test itself
/// deleted the resource (so the drop does not report a spurious 404).
pub struct Cleanup {
    args: Vec<String>,
    armed: bool,
}

impl Cleanup {
    pub fn new(args: &[&str]) -> Self {
        Self {
            args: args.iter().map(|s| s.to_string()).collect(),
            armed: true,
        }
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut args = self.args.clone();
        args.push("--yes".to_string());
        match cx().args(&args).output() {
            Ok(out) if out.status.success() => {}
            Ok(out) => eprintln!(
                "[e2e] cleanup `cx {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            ),
            Err(e) => eprintln!("[e2e] cleanup `cx {}` could not run: {e}", args.join(" ")),
        }
    }
}

/// Run `cx <args>` and return the raw output without asserting on the
/// exit status. Echoes stdout/stderr like [`run_ok`].
pub fn run_raw(args: &[&str]) -> Output {
    let output = cx().args(args).output().expect("failed to run cx");
    println!("\n$ cx {}", args.join(" "));
    if !output.stdout.is_empty() {
        println!("--- stdout ---");
        println!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        println!("--- stderr ---");
        println!("{}", String::from_utf8_lossy(&output.stderr));
    }
    output
}

/// Run `cx <args>` and assert it exits non-zero. Returns stderr as text.
pub fn run_fail(args: &[&str]) -> String {
    let output = run_raw(args);
    assert!(
        !output.status.success(),
        "expected `cx {}` to fail, but it exited 0",
        args.join(" ")
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Run a write command (`--yes` is appended) and parse stdout as JSON.
pub fn run_write_json(args: &[&str]) -> Value {
    let mut full: Vec<&str> = args.to_vec();
    full.push("--yes");
    run_ok_json(&full)
}

/// Run a write command (`--yes` is appended) and assert success; stdout may
/// be empty.
pub fn run_write_ok(args: &[&str]) -> Vec<u8> {
    let mut full: Vec<&str> = args.to_vec();
    full.push("--yes");
    run_ok(&full)
}

/// Extract a resource id from a create/get response. Checks `id` at the
/// top level, then common wrappers (`<any single key>.id`), then the
/// first element of a top-level array. Numeric ids are stringified.
pub fn id_from_response(v: &Value) -> Option<String> {
    fn as_id(x: &Value) -> Option<String> {
        match x {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }
    if let Some(id) = v.get("id").and_then(as_id) {
        return Some(id);
    }
    if let Some(obj) = v.as_object() {
        for inner in obj.values() {
            if let Some(id) = inner.get("id").and_then(as_id) {
                return Some(id);
            }
        }
    }
    if let Some(first) = v.as_array().and_then(|a| a.first()) {
        return id_from_response(first);
    }
    None
}

/// Find the element of a JSON array whose `name_key` equals `name`.
pub fn find_by_name<'a>(list: &'a Value, name_key: &str, name: &str) -> Option<&'a Value> {
    list.as_array()?
        .iter()
        .find(|item| item.get(name_key).and_then(|n| n.as_str()) == Some(name))
}

/// Retry `f` until it returns `Some`, sleeping `delay` between attempts, at
/// most `attempts` times. Use for eventually consistent list endpoints.
pub fn eventually<T>(
    attempts: u32,
    delay: Duration,
    mut f: impl FnMut() -> Option<T>,
) -> Option<T> {
    for i in 0..attempts {
        if let Some(v) = f() {
            return Some(v);
        }
        if i + 1 < attempts {
            std::thread::sleep(delay);
        }
    }
    None
}

/// True when a list item is a temporary resource of some e2e run (its name
/// starts with [`E2E_NAME_PREFIX`]). Discovery helpers skip these so a
/// read-only test never picks a resource another run is about to delete.
pub fn is_e2e_item(item: &Value) -> bool {
    ["name", "display_name", "displayName", "title"]
        .iter()
        .filter_map(|k| item.get(*k).and_then(Value::as_str))
        .any(|n| n.starts_with(E2E_NAME_PREFIX))
}
