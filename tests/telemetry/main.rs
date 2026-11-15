//! Integration tests for client telemetry: the real `cx` binary in a sealed
//! home dir, inspected through `CX_TELEMETRY=log` (events on stderr) or the
//! spool file.

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PREFIX: &str = "cx telemetry: ";
/// Nothing listens on the discard port, so sends fail fast.
const DEAD_ENDPOINT: &str = "http://127.0.0.1:9/v1/logs";

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
        "CX_TELEMETRY_ENDPOINT",
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

#[test]
fn flags_from_env_vars_are_not_reported() {
    let home = temp_home();
    let r = run(log_mode(&home)
        .env("CX_PROFILE", "from-env")
        .args(["dataprime", "list"]));
    assert_eq!(r.command_event()["cx.command.flags"], "");
}

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

#[tokio::test]
async fn api_command_outcomes() {
    let cases = [
        (200, "success", "none", "2xx"),
        (401, "error", "auth", "4xx"),
        (500, "error", "api", "5xx"),
    ];
    for (status, outcome, class, status_class) in cases {
        let server = MockServer::start().await;
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
    let opt_outs: [(&str, &str, bool); 3] = [
        ("CX_TELEMETRY", "0", false),
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
        cmd.env("CX_TELEMETRY_ENDPOINT", DEAD_ENDPOINT);
        if !var.is_empty() {
            cmd.env(var, value);
        }
        let r = run(cmd.args(["dataprime", "list"]));
        assert_eq!(r.code, 0);
        assert!(
            !spool(&home).exists(),
            "{var}={value} config_off={config_off}"
        );
        assert!(!r.stderr.contains("collects usage data"));
    }
}

#[test]
fn no_endpoint_means_off() {
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
        .env("CX_TELEMETRY_ENDPOINT", DEAD_ENDPOINT)
        .args(["dataprime", "list"]));
    let mut second = cx(&home);
    let second = run(second
        .env("CX_TELEMETRY_ENDPOINT", DEAD_ENDPOINT)
        .args(["dataprime", "list"]));

    assert_eq!(first.code, 0);
    assert!(
        first.stderr.contains("cx collects usage data"),
        "{}",
        first.stderr
    );
    assert!(!second.stderr.contains("collects usage data"));
    assert!(!first.stdout.is_empty(), "command output is unchanged");

    let lines: Vec<Value> = fs::read_to_string(spool(&home))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let names: Vec<&str> = lines
        .iter()
        .map(|e| e["event.name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["cli.first_run", "cli.command", "cli.command"]);
}

// ── Flush ─────────────────────────────────────────────────────────────────────

/// A mock serving the alerts API and a collector at `/v1/logs` answering
/// `collector`.
async fn api_and_collector(collector: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/alerts/alerts/v3"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "alertDefs": [] })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/logs"))
        .respond_with(collector)
        .mount(&server)
        .await;
    server
}

async fn collector_requests(server: &MockServer) -> Vec<wiremock::Request> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.url.path() == "/v1/logs")
        .collect()
}

fn on_mode(home: &Path, server: &MockServer) -> Command {
    let mut cmd = cx(home);
    cmd.env("CX_TELEMETRY_ENDPOINT", format!("{}/v1/logs", server.uri()));
    cmd
}

fn spool_lines(home: &Path) -> Vec<Value> {
    fs::read_to_string(spool(home))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn sending_files(home: &Path) -> usize {
    fs::read_dir(home.join(".cx"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".sending")
        })
        .count()
}

fn record_names(body: &Value) -> Vec<String> {
    body["resourceLogs"][0]["scopeLogs"][0]["logRecords"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            r["attributes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|kv| kv["key"] == "event.name")
                .unwrap()["value"]["stringValue"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn network_command_posts_otlp_and_empties_spool() {
    let server = api_and_collector(ResponseTemplate::new(200)).await;
    let home = temp_home();
    write_profile(&home, "mock", &server.uri(), "");

    let r = run(on_mode(&home, &server).args(["alerts", "list"]));
    assert_eq!(r.code, 0, "{}", r.stderr);

    let requests = collector_requests(&server).await;
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert!(req.headers.get("authorization").is_none(), "no auth header");
    assert!(req.headers["user-agent"]
        .to_str()
        .unwrap()
        .starts_with("cx-cli/"));
    let body: Value = serde_json::from_slice(&req.body).unwrap();
    let resource = &body["resourceLogs"][0]["resource"]["attributes"];
    assert!(resource.as_array().unwrap().contains(
        &serde_json::json!({ "key": "service.name", "value": { "stringValue": "cx-cli" } })
    ));
    assert_eq!(record_names(&body), ["cli.first_run", "cli.command"]);

    assert!(!spool(&home).exists(), "spool emptied");
    assert_eq!(sending_files(&home), 0);
}

#[tokio::test]
async fn any_http_status_ends_the_batch() {
    let server = api_and_collector(ResponseTemplate::new(500)).await;
    let home = temp_home();
    write_profile(&home, "mock", &server.uri(), "");

    let r = run(on_mode(&home, &server).args(["alerts", "list"]));
    assert_eq!(r.code, 0);
    assert_eq!(collector_requests(&server).await.len(), 1);
    assert!(
        spool_lines(&home).is_empty(),
        "rejected batch is not retried"
    );
}

#[tokio::test]
async fn slow_collector_never_delays_the_command() {
    let server =
        api_and_collector(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(10)))
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
    let names: Vec<Value> = spool_lines(&home)
        .into_iter()
        .map(|e| e["event.name"].clone())
        .collect();
    assert_eq!(names, ["cli.first_run", "cli.command"], "events kept");
    assert_eq!(sending_files(&home), 0);
}

#[tokio::test]
async fn local_commands_never_send() {
    let server = api_and_collector(ResponseTemplate::new(200)).await;
    let home = temp_home();
    let r = run(on_mode(&home, &server).args(["dataprime", "list"]));
    assert_eq!(r.code, 0);
    assert!(collector_requests(&server).await.is_empty());
    assert_eq!(spool_lines(&home).len(), 2);
}

#[tokio::test]
async fn one_request_carries_at_most_50_events() {
    let server = api_and_collector(ResponseTemplate::new(200)).await;
    let home = temp_home();
    write_profile(&home, "mock", &server.uri(), "");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let seeded: String = (0..60u64)
        .map(|i| {
            format!(
                "{{\"time_unix_nano\":{},\"event.name\":\"cli.first_run\",\"cx.invocation.id\":\"{}\",\"cx.agent\":\"direct\",\"cx.ci\":false,\"cx.tty\":false}}\n",
                now - 1_000_000 + i,
                uuid::Uuid::new_v4()
            )
        })
        .collect();
    fs::write(spool(&home), seeded).unwrap();

    let r = run(on_mode(&home, &server).args(["alerts", "list"]));
    assert_eq!(r.code, 0);
    let requests = collector_requests(&server).await;
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(record_names(&body).len(), 50);
    // 60 seeded + this run's first_run and command, minus 50 sent.
    assert_eq!(spool_lines(&home).len(), 12);
}

// ── Funnel and lifecycle events ───────────────────────────────────────────────

fn onboarding_steps(r: &Run) -> Vec<String> {
    r.named("cli.onboarding")
        .iter()
        .map(|e| e["cx.onboarding.step"].as_str().unwrap().to_string())
        .collect()
}

#[cfg(unix)]
#[tokio::test]
async fn init_records_the_onboarding_funnel_in_order() {
    use std::os::unix::fs::PermissionsExt;
    let server = MockServer::start().await;
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
        command["cx.http.status_class"], "2xx",
        "init is a network command"
    );
}

#[tokio::test]
async fn first_query_succeeded_is_recorded_once() {
    let server = MockServer::start().await;
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

#[tokio::test]
async fn yes_on_a_confirmed_command_records_skipped_yes() {
    let server = MockServer::start().await;
    let home = temp_home();
    write_profile(&home, "mock", &server.uri(), "");

    let r = run(log_mode(&home).args(["alerts", "delete", "abc", "--yes"]));
    let confirms = r.named("cli.confirm");
    assert_eq!(confirms.len(), 1, "{}", r.stderr);
    assert_eq!(confirms[0]["cx.confirm.result"], "skipped_yes");
}

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
        .env("CX_TELEMETRY_ENDPOINT", DEAD_ENDPOINT)
        .args(["schema"]));
    assert!(!spool(&home).exists(), "{}", r.stderr);
    assert!(!r.stderr.contains("collects usage data"));
}

#[test]
fn upgrading_an_existing_install_is_not_a_first_run() {
    let home = temp_home();
    // A state file written by a version before telemetry: no installation id.
    write_state(&home, serde_json::json!({ "latest_binary": null }));
    let r = run(log_mode(&home).args(["dataprime", "list"]));
    assert!(r.named("cli.first_run").is_empty(), "{}", r.stderr);
    assert_eq!(r.command_event()["cx.command.path"], "dataprime list");
}

#[test]
fn mistyped_profile_is_a_usage_error() {
    let home = temp_home();
    write_profile(&home, "real", "http://127.0.0.1:1", "");
    let r = run(log_mode(&home).args(["--profile", "nope", "alerts", "list"]));
    assert_eq!(r.code, 1);
    assert_eq!(r.command_event()["cx.error.class"], "usage");
}
