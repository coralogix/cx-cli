//! Integration tests for client telemetry: the real `cx` binary in a sealed
//! home dir, inspected through `CX_TELEMETRY=log` (events on stderr, debug
//! builds only), the spool file, or a mock key route and ingress.

// The log-mode tests and their helpers exist only in debug builds.
#![cfg_attr(not(debug_assertions), allow(dead_code))]

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PREFIX: &str = "cx telemetry: ";
/// Nothing listens on the discard port, so the key fetch fails fast.
const DEAD_KEY_URL: &str = "http://127.0.0.1:9/key";
const KEY_ROUTE: &str = "/api/v2/cli-telemetry-key";
const TEST_KEY: &str = "cxtp_TestKeyNeverPrinted";

fn temp_home() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cx_test_telemetry_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join("project")).unwrap();
    dir
}

/// `cx` sealed off from the developer's home, credentials and telemetry
/// settings.
fn cx(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("HOME", home)
        .env("CX_HOME", home)
        .env("CX_NO_UPDATE_NOTIFIER", "1")
        .current_dir(home.join("project"));
    for var in [
        "CX_API_KEY",
        "CX_REGION",
        "CX_PROFILE",
        "CX_TELEMETRY",
        "CX_TELEMETRY_KEY_URL",
        "DO_NOT_TRACK",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(cmd: &mut Command) -> Run {
    let out = cmd.output().unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

impl Run {
    fn events(&self) -> Vec<Value> {
        self.stderr
            .lines()
            .filter_map(|l| l.strip_prefix(PREFIX))
            .map(|json| serde_json::from_str(json).expect("telemetry line is JSON"))
            .collect()
    }

    fn named(&self, name: &str) -> Vec<Value> {
        self.events()
            .into_iter()
            .filter(|e| e["event.name"] == name)
            .collect()
    }

    fn command_event(&self) -> Value {
        let events = self.named("cli.command");
        assert_eq!(events.len(), 1, "one cli.command event: {}", self.stderr);
        events.into_iter().next().unwrap()
    }

    /// stderr without the telemetry lines: what the user sees anyway.
    fn plain_stderr(&self) -> String {
        self.stderr
            .lines()
            .filter(|l| !l.starts_with(PREFIX))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(debug_assertions)]
fn log_mode(home: &Path) -> Command {
    let mut cmd = cx(home);
    cmd.env("CX_TELEMETRY", "log");
    cmd
}

fn write_profile(home: &Path, name: &str, base_url: &str, extra: &str) {
    let dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&dir).unwrap();
    let content = format!(
        "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"test-key\"\nregion = \"{base_url}\"\n{extra}"
    );
    fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    fs::write(
        home.join(".cx").join("config.toml"),
        format!("default_profile = \"{name}\"\n"),
    )
    .unwrap();
}

fn spool(home: &Path) -> PathBuf {
    home.join(".cx").join("telemetry-spool.jsonl")
}

// ── cli.command ───────────────────────────────────────────────────────────────

#[cfg(debug_assertions)]
#[test]
fn local_command_emits_one_command_event_without_values() {
    let home = temp_home();
    let r = run(log_mode(&home).args(["dataprime", "list", "--name", "SENTINEL_SECRET"]));
    assert_eq!(r.code, 0, "{}", r.stderr);

    let event = r.command_event();
    assert_eq!(event["cx.command.path"], "dataprime list");
    assert_eq!(event["cx.command.flags"], "--name");
    assert_eq!(event["cx.outcome"], "success");
    assert_eq!(event["cx.http.status_class"], "none");
    assert_eq!(event["service.name"], "cx-cli");
    assert!(event["cx.duration_ms"].as_u64().is_some());
    assert!(
        r.events()
            .iter()
            .all(|e| !e.to_string().contains("SENTINEL_SECRET")),
        "flag value leaked into telemetry"
    );
}

#[cfg(debug_assertions)]
#[test]
fn flags_from_env_vars_are_not_reported() {
    let home = temp_home();
    let r = run(log_mode(&home)
        .env("CX_PROFILE", "from-env")
        .args(["dataprime", "list"]));
    assert_eq!(r.command_event()["cx.command.flags"], "");
}

#[cfg(debug_assertions)]
#[test]
fn no_profile_guidance_is_unchanged_and_recorded() {
    let home = temp_home();
    let r = run(log_mode(&home).args(["logs", "source logs | limit 1"]));
    assert_eq!(r.code, 1);
    assert_eq!(
        r.plain_stderr(),
        "No Coralogix profile is configured.\nRun `cx init` to set up a profile and get started."
    );
    let event = r.command_event();
    assert_eq!(event["cx.command.path"], "logs");
    assert_eq!(event["cx.outcome"], "error");
    assert_eq!(event["cx.error.class"], "usage");
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn api_command_outcomes() {
    let cases = [
        (200, "success", "none", "2xx"),
        (401, "error", "auth", "4xx"),
        (500, "error", "api", "5xx"),
    ];
    for (status, outcome, class, status_class) in cases {
        let server = mock_server().await;
        let body = if status == 200 {
            serde_json::json!({ "alertDefs": [] })
        } else {
            serde_json::json!({ "message": "nope" })
        };
        Mock::given(method("GET"))
            .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;

        let home = temp_home();
        write_profile(&home, "mock", &server.uri(), "cached_team_id = 4015532\n");
        let r = run(log_mode(&home).args(["alerts", "list", "-o", "json"]));

        let event = r.command_event();
        assert_eq!(event["cx.command.path"], "alerts list", "{status}");
        assert_eq!(event["cx.outcome"], outcome, "{status}: {}", r.stderr);
        assert_eq!(event["cx.error.class"], class, "{status}");
        assert_eq!(event["cx.http.status_class"], status_class, "{status}");
        assert_eq!(event["cx.output.format"], "json");
        assert_eq!(event["cx.profile.count"], 1);
        assert_eq!(event["cx.team.id"], "4015532");
    }
}

// ── cli.parse_error ───────────────────────────────────────────────────────────

#[cfg(debug_assertions)]
#[test]
fn parse_errors_keep_clap_output_and_exit_code() {
    for (args, kind) in [
        (vec!["alertz", "list"], "unknown_command"),
        (vec!["alerts", "list", "--nope"], "unknown_flag"),
    ] {
        let home = temp_home();
        let logged = run(log_mode(&home).args(&args));
        let plain = run(cx(&home).args(&args));

        assert_eq!(logged.code, plain.code, "{args:?}");
        assert_ne!(plain.code, 0);
        assert_eq!(logged.plain_stderr(), plain.stderr.trim_end(), "{args:?}");
        let events = logged.named("cli.parse_error");
        assert_eq!(events.len(), 1, "{args:?}: {}", logged.stderr);
        assert_eq!(events[0]["cx.parse.kind"], kind);
        assert!(logged.named("cli.command").is_empty());
    }
}

#[cfg(debug_assertions)]
#[test]
fn help_and_version_are_not_parse_errors() {
    let home = temp_home();
    for args in [vec!["--help"], vec!["--version"], vec!["alerts", "--help"]] {
        let r = run(log_mode(&home).args(&args));
        assert_eq!(r.code, 0, "{args:?}");
        assert!(r.named("cli.parse_error").is_empty(), "{args:?}");
        assert!(r.named("cli.command").is_empty(), "{args:?}");
    }
}

// ── cli.first_run ─────────────────────────────────────────────────────────────

#[cfg(debug_assertions)]
#[test]
fn first_run_is_recorded_once_per_install() {
    let home = temp_home();
    let first = run(log_mode(&home).args(["dataprime", "list"]));
    assert_eq!(first.named("cli.first_run").len(), 1, "{}", first.stderr);
    let second = run(log_mode(&home).args(["dataprime", "list"]));
    assert!(second.named("cli.first_run").is_empty());

    let ids: Vec<Value> = [first.command_event(), second.command_event()]
        .iter()
        .map(|e| e["cx.install.id"].clone())
        .collect();
    assert_eq!(ids[0], ids[1], "install id is stable");
}

// ── Consent ───────────────────────────────────────────────────────────────────

#[test]
fn opt_outs_build_nothing() {
    let opt_outs: [(&str, &str, bool); 4] = [
        ("CX_TELEMETRY", "0", false),
        ("CX_TELEMETRY", "disabled", false),
        ("DO_NOT_TRACK", "1", false),
        ("", "", true), // telemetry = false in config.toml
    ];
    for (var, value, config_off) in opt_outs {
        let home = temp_home();
        if config_off {
            fs::create_dir_all(home.join(".cx")).unwrap();
            fs::write(home.join(".cx").join("config.toml"), "telemetry = false\n").unwrap();
        }
        let mut cmd = cx(&home);
        cmd.env("CX_TELEMETRY_KEY_URL", DEAD_KEY_URL);
        if !var.is_empty() {
            cmd.env(var, value);
        }
        let r = run(cmd.args(["dataprime", "list"]));
        assert_eq!(r.code, 0);
        assert!(
            !spool(&home).exists(),
            "{var}={value} config_off={config_off}"
        );
        // The parent's lock check creates this file, so no check, no sender.
        assert!(!home.join(".cx").join("telemetry-sender.lock").exists());
        assert!(!r.stderr.contains("collects usage data"));
    }
}

#[test]
fn no_key_url_means_off() {
    let home = temp_home();
    let r = run(cx(&home).args(["dataprime", "list"]));
    assert_eq!(r.code, 0);
    assert!(!spool(&home).exists());
    assert!(r.stderr.is_empty(), "{}", r.stderr);
}

#[test]
fn on_mode_prints_notice_once_and_spools_local_commands() {
    let home = temp_home();
    let mut first = cx(&home);
    let first = run(first
        .env("CX_TELEMETRY_KEY_URL", DEAD_KEY_URL)
        .args(["dataprime", "list"]));
    // The first run's sender fails, puts the events back and backs off.
    assert!(
        wait_until(|| backoff(&home).exists()),
        "sender never backed off"
    );
    // So the second run starts no sender, and nothing touches the spool.
    let mut second = cx(&home);
    let second = run(second
        .env("CX_TELEMETRY_KEY_URL", DEAD_KEY_URL)
        .args(["dataprime", "list"]));

    assert_eq!(first.code, 0);
    assert!(
        first.stderr.contains("cx collects usage data"),
        "{}",
        first.stderr
    );
    assert!(!second.stderr.contains("collects usage data"));
    assert!(!first.stdout.is_empty(), "command output is unchanged");

    let lines = spool_lines(&home);
    let names: Vec<&str> = lines
        .iter()
        .map(|e| e["event.name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["cli.first_run", "cli.command", "cli.command"]);
}

// ── Sender ────────────────────────────────────────────────────────────────────

/// The key route's answer: `key`, and the mock itself as the ingress host.
fn key_answer(server: &MockServer, key: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_json(serde_json::json!({ "key": key, "host_url": server.uri() }))
}

async fn mount_key_route(server: &MockServer, answer: impl wiremock::Respond + 'static) {
    Mock::given(method("GET"))
        .and(path(KEY_ROUTE))
        .respond_with(answer)
        .mount(server)
        .await;
}

async fn mount_ingress(server: &MockServer, answer: impl wiremock::Respond + 'static) {
    Mock::given(method("POST"))
        .and(path("/v1/logs"))
        .respond_with(answer)
        .mount(server)
        .await;
}

/// A mock server of this test's own. `MockServer::start()` hands out pooled
/// servers, and a detached sender left by an earlier test would post to the
/// next test that got the same one.
async fn mock_server() -> MockServer {
    MockServer::builder().start().await
}

/// A mock serving the alerts API, with nothing mounted for telemetry yet.
async fn api() -> MockServer {
    let server = mock_server().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "alertDefs": [] })),
        )
        .mount(&server)
        .await;
    server
}

/// A mock serving the alerts API, the key route (answering [`TEST_KEY`]) and
/// an ingress at `/v1/logs` answering `ingress`.
async fn api_and_ingress(ingress: impl wiremock::Respond + 'static) -> MockServer {
    let server = api().await;
    mount_key_route(&server, key_answer(&server, TEST_KEY)).await;
    mount_ingress(&server, ingress).await;
    server
}

async fn requests_to(server: &MockServer, route: &str) -> Vec<wiremock::Request> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.url.path() == route)
        .collect()
}

async fn ingress_requests(server: &MockServer) -> Vec<wiremock::Request> {
    requests_to(server, "/v1/logs").await
}

fn bearer(request: &wiremock::Request) -> String {
    request.headers["authorization"]
        .to_str()
        .unwrap()
        .to_string()
}

/// Every record the ingress received, as `(event.name, cx.invocation.id)`.
async fn ingress_records(server: &MockServer) -> Vec<(String, String)> {
    let attr = |record: &Value, key: &str| {
        record["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|kv| kv["key"] == key)
            .unwrap()["value"]["stringValue"]
            .as_str()
            .unwrap()
            .to_string()
    };
    ingress_requests(server)
        .await
        .iter()
        .flat_map(|req| {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            body["resourceLogs"][0]["scopeLogs"][0]["logRecords"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| (attr(r, "event.name"), attr(r, "cx.invocation.id")))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn on_mode(home: &Path, server: &MockServer) -> Command {
    let mut cmd = cx(home);
    cmd.env(
        "CX_TELEMETRY_KEY_URL",
        format!("{}{KEY_ROUTE}", server.uri()),
    );
    cmd
}

/// The sender sub-process, run in the foreground.
fn sender(home: &Path, server: &MockServer) -> Command {
    let mut cmd = on_mode(home, server);
    cmd.arg("__telemetry-send");
    cmd
}

fn backoff(home: &Path) -> PathBuf {
    home.join(".cx").join("telemetry-backoff")
}

fn key_cache(home: &Path) -> PathBuf {
    home.join(".cx").join("telemetry-key.json")
}

fn spool_lines(home: &Path) -> Vec<Value> {
    fs::read_to_string(spool(home))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn sending_files(home: &Path) -> Vec<PathBuf> {
    fs::read_dir(home.join(".cx"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".sending"))
        .collect()
}

/// Events on disk, in the spool or claimed by a sender. A set, because a
/// rename between the two reads would count a line twice.
fn kept_events(home: &Path) -> std::collections::BTreeSet<String> {
    let mut files = vec![spool(home)];
    files.extend(sending_files(home));
    files
        .iter()
        .flat_map(|f| {
            fs::read_to_string(f)
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Poll for something the detached sender does, for up to 10 s.
fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if condition() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

async fn wait_for_requests(server: &MockServer, count: usize) {
    for _ in 0..200 {
        if ingress_requests(server).await.len() >= count {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the ingress never got {count} request(s)");
}

fn seed_spool(home: &Path, count: u64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let seeded: String = (0..count)
        .map(|i| {
            format!(
                "{{\"time_unix_nano\":{},\"event.name\":\"cli.first_run\",\"cx.invocation.id\":\"{}\",\"cx.agent\":\"direct\",\"cx.ci\":false,\"cx.tty\":false}}\n",
                now - 1_000_000 + i,
                uuid::Uuid::new_v4()
            )
        })
        .collect();
    fs::create_dir_all(home.join(".cx")).unwrap();
    fs::write(spool(home), seeded).unwrap();
}

#[tokio::test]
async fn local_command_delivers_its_events() {
    let server = api_and_ingress(ResponseTemplate::new(200)).await;
    let home = temp_home();

    let r = run(on_mode(&home, &server).args(["dataprime", "list"]));
    assert_eq!(r.code, 0, "{}", r.stderr);
    wait_for_requests(&server, 1).await;

    let requests = ingress_requests(&server).await;
    let req = &requests[0];
    assert_eq!(bearer(req), format!("Bearer {TEST_KEY}"));
    assert_eq!(req.headers["content-type"], "application/json");
    assert!(req.headers["user-agent"]
        .to_str()
        .unwrap()
        .starts_with("cx-cli/"));
    let body: Value = serde_json::from_slice(&req.body).unwrap();
    let resource = &body["resourceLogs"][0]["resource"]["attributes"];
    for (key, value) in [
        ("service.name", "cx-cli"),
        ("cx.application.name", "cx-cli"),
        ("cx.subsystem.name", "usage"),
    ] {
        assert!(resource
            .as_array()
            .unwrap()
            .contains(&serde_json::json!({ "key": key, "value": { "stringValue": value } })));
    }
    let names: Vec<String> = ingress_records(&server)
        .await
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, ["cli.first_run", "cli.command"]);

    assert!(
        wait_until(|| kept_events(&home).is_empty()),
        "spool emptied"
    );
}

/// A fresh backoff file stops senders. One older than 15 minutes does not,
/// and the batch that is then delivered removes it.
#[tokio::test]
async fn backoff_expires_and_a_delivered_batch_clears_it() {
    let server = api_and_ingress(ResponseTemplate::new(200)).await;
    let home = temp_home();
    seed_spool(&home, 1);
    fs::write(backoff(&home), "").unwrap();

    let r = run(on_mode(&home, &server).args(["dataprime", "list"]));
    assert_eq!(r.code, 0, "{}", r.stderr);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(
        ingress_requests(&server).await.is_empty(),
        "a sender ran while backing off"
    );
    assert!(backoff(&home).exists());

    let expired = std::time::SystemTime::now() - std::time::Duration::from_secs(16 * 60);
    fs::File::options()
        .write(true)
        .open(backoff(&home))
        .unwrap()
        .set_modified(expired)
        .unwrap();
    let r = run(on_mode(&home, &server).args(["dataprime", "list"]));
    assert_eq!(r.code, 0, "{}", r.stderr);
    wait_for_requests(&server, 1).await;
    assert!(
        wait_until(|| !backoff(&home).exists() && kept_events(&home).is_empty()),
        "backoff file not cleared after a delivered batch"
    );
}

#[tokio::test]
async fn parse_error_is_delivered() {
    let server = api_and_ingress(ResponseTemplate::new(200)).await;
    let home = temp_home();
    let r = run(on_mode(&home, &server).args(["alertz", "list"]));
    assert_ne!(r.code, 0);
    wait_for_requests(&server, 1).await;
    let names: Vec<String> = ingress_records(&server)
        .await
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, ["cli.first_run", "cli.parse_error"]);
}

#[tokio::test]
async fn hung_ingress_never_delays_the_command() {
    let server =
        api_and_ingress(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(60)))
            .await;
    let home = temp_home();
    write_profile(&home, "mock", &server.uri(), "");

    let started = std::time::Instant::now();
    let r = run(on_mode(&home, &server).args(["alerts", "list"]));
    let elapsed = started.elapsed();

    assert_eq!(r.code, 0);
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "took {elapsed:?}"
    );
    wait_for_requests(&server, 1).await;
    assert_eq!(kept_events(&home).len(), 2, "events kept while unanswered");
}

#[tokio::test]
async fn sender_status_decides_whether_the_batch_is_kept() {
    for (status, kept) in [
        (200, false),
        (400, false),
        // Not followed: the ingress is asked once and the batch is kept.
        (302, true),
        (408, true),
        (429, true),
        (500, true),
        (502, true),
        (503, true),
    ] {
        // The header only matters for the 302: a followed redirect would show
        // up as a second request.
        let server =
            api_and_ingress(ResponseTemplate::new(status).insert_header("Location", "/v1/logs"))
                .await;
        let home = temp_home();
        seed_spool(&home, 3);

        assert_eq!(run(&mut sender(&home, &server)).code, 0);
        assert_eq!(ingress_requests(&server).await.len(), 1, "{status}");
        assert_eq!(
            spool_lines(&home).len(),
            if kept { 3 } else { 0 },
            "{status}"
        );
        assert_eq!(backoff(&home).exists(), kept, "{status}");
        assert!(sending_files(&home).is_empty(), "{status}");

        // While backing off, a command starts no sender.
        let r = run(on_mode(&home, &server).args(["dataprime", "list"]));
        assert_eq!(r.code, 0);
        if kept {
            std::thread::sleep(std::time::Duration::from_millis(500));
            assert_eq!(ingress_requests(&server).await.len(), 1, "{status}");
        } else {
            wait_for_requests(&server, 2).await;
        }
    }
}

#[tokio::test]
async fn sender_posts_batches_of_50_until_the_spool_is_empty() {
    let server = api_and_ingress(ResponseTemplate::new(200)).await;
    let home = temp_home();
    seed_spool(&home, 120);

    assert_eq!(run(&mut sender(&home, &server)).code, 0);
    let sizes: Vec<usize> = ingress_requests(&server)
        .await
        .iter()
        .map(|req| {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            body["resourceLogs"][0]["scopeLogs"][0]["logRecords"]
                .as_array()
                .unwrap()
                .len()
        })
        .collect();
    assert_eq!(sizes, [50, 50, 20]);
    assert!(kept_events(&home).is_empty());
}

#[tokio::test]
async fn sender_honours_an_opt_out_set_after_the_spawn() {
    let server = api_and_ingress(ResponseTemplate::new(200)).await;
    let home = temp_home();
    seed_spool(&home, 3);

    assert_eq!(run(sender(&home, &server).env("CX_TELEMETRY", "0")).code, 0);
    assert!(ingress_requests(&server).await.is_empty());
    assert_eq!(spool_lines(&home).len(), 3);
}

#[tokio::test]
async fn sender_exits_when_another_holds_the_lock() {
    let server = api_and_ingress(ResponseTemplate::new(200)).await;
    let home = temp_home();
    seed_spool(&home, 3);
    let lock = fs::File::create(home.join(".cx").join("telemetry-sender.lock")).unwrap();
    lock.try_lock().unwrap();

    assert_eq!(run(&mut sender(&home, &server)).code, 0);
    // Nor does a command start one.
    assert_eq!(
        run(on_mode(&home, &server).args(["dataprime", "list"])).code,
        0
    );
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(ingress_requests(&server).await.is_empty());

    drop(lock);
    assert_eq!(run(&mut sender(&home, &server)).code, 0);
    // 3 seeded + the command's first_run and command.
    assert_eq!(ingress_records(&server).await.len(), 5);
}

/// Answers 200 after [`Arrivals::DELAY`] and records when each request came.
struct Arrivals(std::sync::Arc<std::sync::Mutex<Vec<std::time::Instant>>>);

impl Arrivals {
    const DELAY: std::time::Duration = std::time::Duration::from_millis(100);
}

impl wiremock::Respond for Arrivals {
    fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
        self.0.lock().unwrap().push(std::time::Instant::now());
        ResponseTemplate::new(200).set_delay(Self::DELAY)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_runs_send_one_at_a_time_and_each_event_once() {
    let arrivals = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = api_and_ingress(Arrivals(arrivals.clone())).await;
    let home = temp_home();
    let commands = |records: &[(String, String)]| {
        records
            .iter()
            .filter(|(name, _)| name == "cli.command")
            .count()
    };

    // One run first, so the install id exists and `first_run` is out.
    assert_eq!(
        run(on_mode(&home, &server).args(["dataprime", "list"])).code,
        0
    );
    let mut runs = 1;
    wait_for_requests(&server, 1).await;

    let threads: Vec<_> = (0..20)
        .map(|_| {
            let mut cmd = on_mode(&home, &server);
            std::thread::spawn(move || run(cmd.args(["dataprime", "list"])).code)
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap(), 0);
        runs += 1;
    }

    // A run that saw the lock held, just as that sender was leaving, leaves
    // its event for the next run's sender. So run again until all are in.
    'delivered: loop {
        for _ in 0..60 {
            if commands(&ingress_records(&server).await) >= runs {
                break 'delivered;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(runs < 30, "events were never delivered");
        assert_eq!(
            run(on_mode(&home, &server).args(["dataprime", "list"])).code,
            0
        );
        runs += 1;
    }

    let records = ingress_records(&server).await;
    assert_eq!(commands(&records), runs);
    let unique: std::collections::BTreeSet<_> = records.iter().collect();
    assert_eq!(unique.len(), records.len(), "an event arrived twice");

    // A sender waits for each answer, so two requests closer together than
    // the answer delay came from two senders posting at once.
    let arrivals = arrivals.lock().unwrap();
    for pair in arrivals.windows(2) {
        let gap = pair[1].duration_since(pair[0]);
        assert!(
            gap >= Arrivals::DELAY - std::time::Duration::from_millis(5),
            "two senders posted at once ({gap:?} apart)"
        );
    }
}

// ── Key route ─────────────────────────────────────────────────────────────────

/// Answers with the next key in the list on each call, then the last one.
struct Keys {
    host_url: String,
    keys: Vec<&'static str>,
    calls: std::sync::atomic::AtomicUsize,
}

impl wiremock::Respond for Keys {
    fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
        let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let key = self.keys[call.min(self.keys.len() - 1)];
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({ "key": key, "host_url": self.host_url }))
    }
}

/// An ingress that takes only `Bearer <self.0>`.
struct Accepts(&'static str);

impl wiremock::Respond for Accepts {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        if bearer(request) == format!("Bearer {}", self.0) {
            ResponseTemplate::new(200)
        } else {
            ResponseTemplate::new(401)
        }
    }
}

#[tokio::test]
async fn cached_key_is_reused_by_the_next_sender() {
    let server = api_and_ingress(ResponseTemplate::new(200)).await;
    let home = temp_home();

    for _ in 0..2 {
        seed_spool(&home, 3);
        assert_eq!(run(&mut sender(&home, &server)).code, 0);
    }
    assert_eq!(ingress_requests(&server).await.len(), 2);
    assert_eq!(requests_to(&server, KEY_ROUTE).await.len(), 1);
}

#[tokio::test]
async fn no_key_keeps_the_events_and_backs_off() {
    let not_json = ResponseTemplate::new(200).set_body_string("<html>");
    let empty_key =
        ResponseTemplate::new(200).set_body_json(serde_json::json!({ "key": "", "host_url": "" }));
    for (case, answer) in [
        ("not deployed", ResponseTemplate::new(404)),
        ("no key stored", ResponseTemplate::new(503)),
        ("rate limited", ResponseTemplate::new(429)),
        ("not json", not_json),
        ("empty key", empty_key),
    ] {
        let server = api().await;
        mount_key_route(&server, answer).await;
        mount_ingress(&server, ResponseTemplate::new(200)).await;
        let home = temp_home();
        seed_spool(&home, 3);

        let r = run(&mut sender(&home, &server));
        assert_eq!(r.code, 0, "{case}");
        assert!(r.stdout.is_empty() && r.stderr.is_empty(), "{case}");
        assert_eq!(requests_to(&server, KEY_ROUTE).await.len(), 1, "{case}");
        assert!(ingress_requests(&server).await.is_empty(), "{case}");
        assert_eq!(spool_lines(&home).len(), 3, "{case}");
        assert!(backoff(&home).exists(), "{case}");
        assert!(!key_cache(&home).exists(), "{case}");

        // While backing off, a command starts no sender and looks the same.
        let r = run(on_mode(&home, &server).args(["dataprime", "list"]));
        assert_eq!(r.code, 0, "{case}");
        assert!(!r.stdout.is_empty(), "{case}");
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(requests_to(&server, KEY_ROUTE).await.len(), 1, "{case}");
    }
}

#[tokio::test]
async fn a_host_that_is_not_coralogix_is_not_sent_to() {
    let server = api().await;
    let answer = ResponseTemplate::new(200).set_body_json(
        serde_json::json!({ "key": TEST_KEY, "host_url": "https://evil.example.com" }),
    );
    mount_key_route(&server, answer).await;
    mount_ingress(&server, ResponseTemplate::new(200)).await;
    let home = temp_home();
    seed_spool(&home, 3);

    assert_eq!(run(&mut sender(&home, &server)).code, 0);
    assert!(ingress_requests(&server).await.is_empty());
    assert_eq!(spool_lines(&home).len(), 3);
    assert!(backoff(&home).exists());
    assert!(!key_cache(&home).exists());
}

#[tokio::test]
async fn rotated_key_is_fetched_again_and_the_batch_delivered() {
    let server = api().await;
    let keys = Keys {
        host_url: server.uri(),
        keys: vec!["cxtp_old", "cxtp_new"],
        calls: Default::default(),
    };
    mount_key_route(&server, keys).await;
    mount_ingress(&server, Accepts("cxtp_new")).await;
    let home = temp_home();
    seed_spool(&home, 3);

    assert_eq!(run(&mut sender(&home, &server)).code, 0);
    let sent: Vec<String> = ingress_requests(&server).await.iter().map(bearer).collect();
    assert_eq!(sent, ["Bearer cxtp_old", "Bearer cxtp_new"]);
    assert_eq!(requests_to(&server, KEY_ROUTE).await.len(), 2);
    assert!(kept_events(&home).is_empty());
    assert!(!backoff(&home).exists());
    assert!(fs::read_to_string(key_cache(&home))
        .unwrap()
        .contains("cxtp_new"));

    // The next sender starts from the new key.
    seed_spool(&home, 1);
    assert_eq!(run(&mut sender(&home, &server)).code, 0);
    assert_eq!(requests_to(&server, KEY_ROUTE).await.len(), 2);
    assert!(kept_events(&home).is_empty());
}

#[tokio::test]
async fn a_key_refused_twice_keeps_the_batch_and_backs_off() {
    for status in [401, 403] {
        let server = api_and_ingress(ResponseTemplate::new(status)).await;
        let home = temp_home();
        seed_spool(&home, 3);

        assert_eq!(run(&mut sender(&home, &server)).code, 0);
        assert_eq!(ingress_requests(&server).await.len(), 2, "{status}");
        assert_eq!(requests_to(&server, KEY_ROUTE).await.len(), 2, "{status}");
        assert_eq!(spool_lines(&home).len(), 3, "{status}");
        assert!(backoff(&home).exists(), "{status}");
        assert!(sending_files(&home).is_empty(), "{status}");
    }
}

#[tokio::test]
async fn hung_key_route_never_delays_the_command() {
    let server = api().await;
    let hung = key_answer(&server, TEST_KEY).set_delay(std::time::Duration::from_secs(60));
    mount_key_route(&server, hung).await;
    mount_ingress(&server, ResponseTemplate::new(200)).await;
    let home = temp_home();
    write_profile(&home, "mock", &server.uri(), "");

    let started = std::time::Instant::now();
    let r = run(on_mode(&home, &server).args(["alerts", "list"]));
    let elapsed = started.elapsed();

    assert_eq!(r.code, 0);
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "took {elapsed:?}"
    );
    assert_eq!(kept_events(&home).len(), 2, "events kept while unanswered");
    assert!(ingress_requests(&server).await.is_empty());
}

#[tokio::test]
async fn the_key_is_never_printed_or_spooled() {
    // The ingress fails, so the events stay on disk to be looked at.
    let server = api_and_ingress(ResponseTemplate::new(500)).await;
    let home = temp_home();

    let command = run(on_mode(&home, &server).args(["dataprime", "list"]));
    assert!(
        wait_until(|| backoff(&home).exists()),
        "sender never backed off"
    );
    fs::remove_file(backoff(&home)).unwrap();
    let sender = run(&mut sender(&home, &server));

    assert_eq!(ingress_requests(&server).await.len(), 2);
    for output in [
        &command.stdout,
        &command.stderr,
        &sender.stdout,
        &sender.stderr,
    ] {
        assert!(!output.contains(TEST_KEY));
    }
    assert!(sender.stdout.is_empty() && sender.stderr.is_empty());
    assert!(!fs::read_to_string(spool(&home)).unwrap().contains(TEST_KEY));
}

// ── Funnel and lifecycle events ───────────────────────────────────────────────

fn onboarding_steps(r: &Run) -> Vec<String> {
    r.named("cli.onboarding")
        .iter()
        .map(|e| e["cx.onboarding.step"].as_str().unwrap().to_string())
        .collect()
}

#[cfg(unix)]
#[cfg(debug_assertions)]
#[tokio::test]
async fn init_records_the_onboarding_funnel_in_order() {
    use std::os::unix::fs::PermissionsExt;
    let server = mock_server().await;
    let home = temp_home();
    // A fake npx: `skills ls` lists nothing, `skills add` succeeds.
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let npx = bin.join("npx");
    fs::write(
        &npx,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 10.0.0; exit 0; fi\n\
         if [ \"$3\" = \"ls\" ]; then echo '[]'; fi\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&npx, fs::Permissions::from_mode(0o755)).unwrap();

    let r = run(log_mode(&home)
        .env("PATH", &bin)
        .env("CX_CREDENTIAL_STORAGE", "file")
        .args([
            "init",
            "--url",
            &server.uri(),
            "--api-key",
            "k",
            "--global-skills",
        ]));
    assert_eq!(r.code, 0, "{}", r.stderr);

    assert_eq!(
        onboarding_steps(&r),
        ["init_started", "profile_added", "init_completed"]
    );
    let added = &r.named("cli.onboarding")[1];
    assert_eq!(added["cx.auth.method"], "api_key");
    assert_eq!(added["cx.credential.store"], "file");

    let skills = r.named("cli.skills_install");
    assert_eq!(skills.len(), 1, "{}", r.stderr);
    assert_eq!(skills[0]["cx.skills.scope"], "global");
    assert_eq!(skills[0]["cx.outcome"], "success");

    let command = r.command_event();
    assert_eq!(command["cx.command.path"], "init");
    assert_eq!(
        command["cx.http.status_class"], "none",
        "this init got no API response, so there is no status to report"
    );
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn profile_added_is_recorded_for_a_new_profile_only() {
    let server = mock_server().await;
    let home = temp_home();
    let add = |extra: &[&str]| {
        let mut cmd = log_mode(&home);
        cmd.env("CX_CREDENTIAL_STORAGE", "file")
            .args(["profiles", "add", "work", "--api-key", "k", "--url"])
            .arg(server.uri())
            .args(extra);
        run(&mut cmd)
    };

    let first = add(&[]);
    assert_eq!(first.code, 0, "{}", first.stderr);
    assert_eq!(onboarding_steps(&first), ["profile_added"]);

    // Overwriting it is an edit, not a funnel step.
    let again = add(&["--force"]);
    assert_eq!(again.code, 0, "{}", again.stderr);
    assert_eq!(onboarding_steps(&again), [] as [&str; 0]);
    assert_eq!(again.command_event()["cx.command.path"], "profiles add");
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn first_query_succeeded_is_recorded_once() {
    let server = mock_server().await;
    Mock::given(method("GET"))
        .and(path("/metrics/api/v1/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "success",
            "data": { "resultType": "vector", "result": [] }
        })))
        .mount(&server)
        .await;
    let home = temp_home();
    write_profile(&home, "mock", &server.uri(), "");

    let first = run(log_mode(&home).args(["metrics", "query", "up", "-o", "json"]));
    assert_eq!(first.code, 0, "{}", first.stderr);
    assert_eq!(onboarding_steps(&first), ["first_query_succeeded"]);
    let second = run(log_mode(&home).args(["metrics", "query", "up", "-o", "json"]));
    assert!(onboarding_steps(&second).is_empty());
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn yes_on_a_confirmed_command_records_skipped_yes() {
    let server = mock_server().await;
    let home = temp_home();
    write_profile(&home, "mock", &server.uri(), "");

    let r = run(log_mode(&home).args(["alerts", "delete", "abc", "--yes"]));
    let confirms = r.named("cli.confirm");
    assert_eq!(confirms.len(), 1, "{}", r.stderr);
    assert_eq!(confirms[0]["cx.confirm.result"], "skipped_yes");
}

#[cfg(debug_assertions)]
#[test]
fn non_interactive_confirm_is_recorded() {
    let home = temp_home();
    write_profile(&home, "mock", "http://127.0.0.1:1", "");
    // No --yes and no terminal: the prompt is refused before any request.
    let r = run(log_mode(&home).args(["alerts", "delete", "abc"]));
    assert_ne!(r.code, 0);
    let confirms = r.named("cli.confirm");
    assert_eq!(confirms[0]["cx.confirm.result"], "non_interactive");
}

fn write_state(home: &Path, state: Value) {
    fs::create_dir_all(home.join(".cx")).unwrap();
    fs::write(home.join(".cx").join("state.json"), state.to_string()).unwrap();
}

#[cfg(debug_assertions)]
#[test]
fn version_change_is_recorded_once() {
    let home = temp_home();
    write_state(
        &home,
        serde_json::json!({
            "installation_id": "3f0e6c1a-9b7d-4c55-8e21-5a6f0d2b7c90",
            "last_run_version": "0.0.1"
        }),
    );
    let first = run(log_mode(&home).args(["dataprime", "list"]));
    let changed = first.named("cli.version_changed");
    assert_eq!(changed.len(), 1, "{}", first.stderr);
    assert_eq!(changed[0]["cx.version.previous"], "0.0.1");
    assert!(first.named("cli.first_run").is_empty(), "not a new install");

    let second = run(log_mode(&home).args(["dataprime", "list"]));
    assert!(second.named("cli.version_changed").is_empty());
}

#[cfg(debug_assertions)]
#[test]
fn update_notice_is_recorded_with_a_strict_version_only() {
    for (latest, expected) in [("99.0.0", Some("99.0.0")), ("99.0.0-evil text", None)] {
        let home = temp_home();
        write_state(
            &home,
            serde_json::json!({
                "last_checked_at": chrono::Utc::now().to_rfc3339(),
                "latest_binary": latest
            }),
        );
        let r = run(log_mode(&home)
            .env_remove("CX_NO_UPDATE_NOTIFIER")
            .args(["dataprime", "list"]));
        let notices = r.named("cli.update_notice");
        match expected {
            Some(v) => {
                assert_eq!(notices.len(), 1, "{latest}: {}", r.stderr);
                assert_eq!(notices[0]["cx.version.latest"], v);
            }
            None => assert!(notices.is_empty(), "{latest} must not be sent"),
        }
    }
}

// ── Review follow-ups ─────────────────────────────────────────────────────────

#[test]
fn config_opt_out_holds_when_another_key_is_invalid() {
    let home = temp_home();
    fs::create_dir_all(home.join(".cx")).unwrap();
    // read_only must be a bool, so the full config fails to parse.
    fs::write(
        home.join(".cx").join("config.toml"),
        "telemetry = false\nread_only = \"yes\"\n",
    )
    .unwrap();
    let r = run(cx(&home)
        .env("CX_TELEMETRY_KEY_URL", DEAD_KEY_URL)
        .args(["schema"]));
    assert!(!spool(&home).exists(), "{}", r.stderr);
    assert!(!r.stderr.contains("collects usage data"));
}

#[cfg(debug_assertions)]
#[test]
fn upgrading_an_existing_install_is_not_a_first_run() {
    let home = temp_home();
    // A state file written by a version before telemetry: no installation id.
    write_state(&home, serde_json::json!({ "latest_binary": null }));
    let r = run(log_mode(&home).args(["dataprime", "list"]));
    assert!(r.named("cli.first_run").is_empty(), "{}", r.stderr);
    assert_eq!(r.command_event()["cx.command.path"], "dataprime list");
}

#[cfg(debug_assertions)]
#[test]
fn mistyped_profile_is_a_usage_error() {
    let home = temp_home();
    write_profile(&home, "real", "http://127.0.0.1:1", "");
    let r = run(log_mode(&home).args(["--profile", "nope", "alerts", "list"]));
    assert_eq!(r.code, 1);
    assert_eq!(r.command_event()["cx.error.class"], "usage");
}
