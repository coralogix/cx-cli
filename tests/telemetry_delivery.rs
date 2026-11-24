use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cli_otel::Spool;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value::Value, AnyValue, KeyValue};
use opentelemetry_proto::tonic::trace::v1::{span::SpanKind, ResourceSpans, Span};
use prost::Message;
use serde_json::json;
use tokio::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const TRACE_PATH: &str = "/v1/traces";
const KEY_PATH: &str = "/key";
const KEY_CACHE: &str = "telemetry-key.json";
const RETRY_AFTER: &str = "retry-after";
const PROFILE: &str = "queue-sensitive-profile";
const PROFILE_KEY: &str = "queue-sensitive-profile-key";
const COLLECTOR_KEY: &str = "queue-private-collector-key";
const QUERY: &str = "queue-sensitive-query";
const ERROR: &str = "queue-sensitive-api-error";
const RELEASE_NOTICE: &str =
    "cx sends usage telemetry by default. Opt out with CX_TELEMETRY=off, DO_NOT_TRACK=1, or telemetry=false in ~/.cx/config.toml.";

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("cx-telemetry-delivery-{}", uuid::Uuid::new_v4())))
    }

    fn cx_dir(&self) -> PathBuf {
        self.0.join(".cx")
    }

    fn config(&self, contents: &[u8]) {
        fs::create_dir_all(self.cx_dir()).unwrap();
        fs::write(self.cx_dir().join("config.toml"), contents).unwrap();
    }

    fn profile(&self, endpoint: &str) {
        let profiles = self.cx_dir().join("profiles");
        fs::create_dir_all(&profiles).unwrap();
        fs::write(
            profiles.join(format!("{PROFILE}.toml")),
            format!(
                "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"{PROFILE_KEY}\"\nregion = \"{endpoint}\"\n"
            ),
        )
        .unwrap();
    }

    fn cx(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cx"));
        command
            .env_clear()
            .env("CX_HOME", &self.0)
            .env("CX_NO_UPDATE_NOTIFIER", "1")
            .env("CX_NO_SKILLS_AUTO_UPDATE", "1")
            .kill_on_drop(true)
            .args(args);
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        command
    }

    fn background(&self, args: &[&str], endpoint: &str) -> Command {
        let mut command = self.cx(args);
        command
            .env("CX_TELEMETRY_ENDPOINT", endpoint)
            .env("CX_TELEMETRY_DELIVERY", "background")
            .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY);
        command
    }

    fn discovery(&self, args: &[&str], key_url: &str) -> Command {
        let mut command = self.cx(args);
        command
            .env("CX_TELEMETRY_KEY_URL", key_url)
            .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
            .env("CX_API_KEY", PROFILE_KEY);
        command
    }

    fn partition(&self) -> PathBuf {
        let mut dirs = fs::read_dir(self.cx_dir().join("telemetry")).unwrap();
        let path = dirs.next().unwrap().unwrap().path();
        assert!(dirs.next().is_none());
        path
    }

    fn notice_dir(&self) -> PathBuf {
        self.cx_dir().join("telemetry-notice")
    }

    fn notice_marker(&self) -> PathBuf {
        self.notice_dir().join("release-default-notice-shown")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn batches(directory: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "otlp")
        })
        .collect();
    paths.sort();
    paths
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap()
}

async fn wait_for_requests(server: &MockServer, minimum: usize) -> Vec<Request> {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let received = requests(server).await;
        if received.len() >= minimum {
            return received;
        }
        assert!(
            Instant::now() < deadline,
            "received {} requests, expected {minimum}",
            received.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !ready() {
        assert!(Instant::now() < deadline, "sender state did not converge");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_idle(directory: &Path) {
    let spool = Spool::new(directory.to_owned());
    wait_until(|| spool.try_lock().is_ok()).await;
}

fn assert_success(output: Output) {
    assert!(output.status.success(), "cx exited with {}", output.status);
}

fn decode(bytes: &[u8]) -> ResourceSpans {
    let mut data = ExportTraceServiceRequest::decode(bytes).unwrap();
    assert_eq!(data.resource_spans.len(), 1);
    data.resource_spans.pop().unwrap()
}

fn spans(data: &ResourceSpans) -> impl Iterator<Item = &Span> {
    data.scope_spans.iter().flat_map(|scope| &scope.spans)
}

fn attr<'a>(attrs: &'a [KeyValue], key: &str) -> Option<&'a Value> {
    attrs
        .iter()
        .find(|attr| attr.key == key)?
        .value
        .as_ref()?
        .value
        .as_ref()
}

fn string_attr<'a>(attrs: &'a [KeyValue], key: &str) -> Option<&'a str> {
    match attr(attrs, key)? {
        Value::StringValue(value) => Some(value.as_str()),
        _ => None,
    }
}

fn assert_private(bytes: &[u8]) {
    for value in [
        PROFILE,
        PROFILE_KEY,
        COLLECTOR_KEY,
        QUERY,
        ERROR,
        "ci-marker-secret",
        "agent-marker-secret",
    ] {
        assert!(!bytes
            .windows(value.len())
            .any(|window| window == value.as_bytes()));
    }
}

#[tokio::test]
async fn consent_defaults_to_configured_collection_and_opt_outs_win() {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&collector)
        .await;
    let destination = format!("{}{TRACE_PATH}", collector.uri());

    for env in [
        None,
        Some(""),
        Some(" \t"),
        Some("1"),
        Some(" TrUe "),
        Some("ON"),
        Some("yes"),
    ] {
        let home = Sandbox::new();
        home.config(b"telemetry = true\nread_only = 'unrelated type mismatch'\n");
        let mut command = home.cx(&["--version"]);
        command
            .env("CX_TELEMETRY_ENDPOINT", &destination)
            .env("DO_NOT_TRACK", "false");
        if let Some(env) = env {
            command.env("CX_TELEMETRY", env);
        }
        assert_success(command.output().await.unwrap());
        assert!(home.cx_dir().join("state.json").is_file());
    }
    assert_eq!(requests(&collector).await.len(), 7);

    let assert_opted_out = |home: &Sandbox| {
        assert!(!home.cx_dir().join("state.json").exists());
        assert!(!home.cx_dir().join("telemetry").exists());
    };
    for env in ["0", " FaLsE ", " off\t", "no", "log", "unexpected"] {
        let home = Sandbox::new();
        assert_success(
            home.background(&["--version"], &destination)
                .env("CX_TELEMETRY", env)
                .output()
                .await
                .unwrap(),
        );
        assert_opted_out(&home);
    }
    for value in ["1", " True ", "oN", " YES "] {
        let home = Sandbox::new();
        assert_success(
            home.background(&["--version"], &destination)
                .env("CX_TELEMETRY", "on")
                .env("DO_NOT_TRACK", value)
                .output()
                .await
                .unwrap(),
        );
        assert_opted_out(&home);
    }
    for contents in [
        b"telemetry = false\n".as_slice(),
        b"telemetry = false\nread_only = 'unrelated type mismatch'\n",
        b"telemetry = 'false'\n",
        b"telemetry = = true\n",
        &[0xff],
    ] {
        let home = Sandbox::new();
        home.config(contents);
        assert_success(
            home.background(&["--version"], &destination)
                .env("CX_TELEMETRY", "on")
                .output()
                .await
                .unwrap(),
        );
        assert_opted_out(&home);
    }
    let home = Sandbox::new();
    fs::create_dir_all(home.cx_dir().join("config.toml")).unwrap();
    assert_success(
        home.background(&["--version"], &destination)
            .output()
            .await
            .unwrap(),
    );
    assert_opted_out(&home);
    assert_eq!(requests(&collector).await.len(), 7);

    if cfg!(debug_assertions) {
        let home = Sandbox::new();
        assert_success(
            home.cx(&["--version"])
                .env("CX_TELEMETRY", "on")
                .output()
                .await
                .unwrap(),
        );
        assert_opted_out(&home);
    }
}

#[tokio::test]
async fn global_config_updates_preserve_consent_when_another_setting_is_invalid() {
    let collector = MockServer::start().await;
    let endpoint = format!("{}{TRACE_PATH}", collector.uri());
    let home = Sandbox::new();
    home.profile(&collector.uri());
    home.config(b"telemetry = false\nread_only = 'unrelated type mismatch'\n");

    assert_success(
        home.background(&["profiles", "set-default", PROFILE], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    let settings = fs::read_to_string(home.cx_dir().join("config.toml"))
        .unwrap()
        .parse::<toml::Table>()
        .unwrap();
    assert_eq!(settings["default_profile"].as_str(), Some(PROFILE));
    assert_eq!(settings["telemetry"].as_bool(), Some(false));
    assert_success(
        home.background(&["--version"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    assert!(!home.cx_dir().join("state.json").exists());
    assert!(!home.cx_dir().join("telemetry").exists());
    assert!(requests(&collector).await.is_empty());
}

#[tokio::test]
async fn inline_delivery_drops_running_command_when_consent_is_revoked_before_shutdown() {
    let home = Sandbox::new();
    let api = MockServer::start().await;
    home.profile(&api.uri());
    home.config(b"telemetry = true\n");
    let (arrived_tx, arrived_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(Some(release_rx)));
    Mock::given(method("GET"))
        .and(path("/metrics/api/v1/query"))
        .respond_with({
            let release_rx = Arc::clone(&release_rx);
            move |_: &Request| {
                let _ = arrived_tx.send(());
                if let Some(receiver) = release_rx.lock().unwrap().as_ref() {
                    let _ = receiver.recv_timeout(Duration::from_secs(5));
                }
                ResponseTemplate::new(200).set_body_json(json!({
                    "status": "success",
                    "data": { "resultType": "vector", "result": [] }
                }))
            }
        })
        .mount(&api)
        .await;
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&collector)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/logs"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&collector)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", collector.uri());

    let child = home
        .cx(&[
            "metrics",
            "query",
            "up",
            "-p",
            PROFILE,
            "--no-console-link",
            "-o",
            "json",
        ])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .spawn()
        .unwrap();
    arrived_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    home.config(b"telemetry = false\n");
    release_tx.send(()).unwrap();
    assert_success(child.wait_with_output().await.unwrap());
    assert!(requests(&collector).await.is_empty());
}

#[tokio::test]
async fn replay_retains_original_bytes_and_obeys_current_consent() {
    let home = Sandbox::new();
    let api = MockServer::start().await;
    home.profile(&api.uri());
    Mock::given(method("GET"))
        .and(path("/metrics/api/v1/query"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"message": ERROR})))
        .mount(&api)
        .await;

    let collector = MockServer::start().await;
    let online = Arc::new(AtomicBool::new(false));
    let opt_out_next = Arc::new(AtomicBool::new(true));
    let config_path = home.cx_dir().join("config.toml");
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with({
            let online = Arc::clone(&online);
            move |_: &Request| {
                if !online.load(Ordering::SeqCst) {
                    return ResponseTemplate::new(503);
                }
                if opt_out_next.swap(false, Ordering::SeqCst) {
                    fs::write(&config_path, "telemetry = false\n").unwrap();
                }
                ResponseTemplate::new(200)
            }
        })
        .mount(&collector)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", collector.uri());

    let failed = home
        .background(
            &[
                "metrics",
                "query",
                QUERY,
                "-p",
                PROFILE,
                "--no-console-link",
                "-o",
                "json",
            ],
            &endpoint,
        )
        .env("GITHUB_ACTIONS", "ci-marker-secret")
        .env("CLAUDE_CODE", "agent-marker-secret")
        .env(
            "TRACEPARENT",
            "00-11111111111111111111111111111111-2222222222222222-01",
        )
        .output()
        .await
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    let directory = home.partition();
    let first_paths = batches(&directory);
    assert_eq!(first_paths.len(), 1);
    let queued = fs::read(&first_paths[0]).unwrap();
    assert_private(&queued);
    let original = decode(&queued);
    let resource = &original.resource.as_ref().unwrap().attributes;
    assert_eq!(
        string_attr(resource, "service.version"),
        Some(env!("CARGO_PKG_VERSION"))
    );
    let root = spans(&original)
        .find(|span| span.kind == SpanKind::Internal as i32)
        .unwrap();
    let http = spans(&original)
        .find(|span| span.kind == SpanKind::Client as i32)
        .unwrap();
    assert_eq!(root.trace_id, [0x11; 16]);
    assert_eq!(root.parent_span_id, [0x22; 8]);
    assert_eq!(http.parent_span_id, root.span_id);
    assert_eq!(http.trace_id, root.trace_id);
    assert_eq!(
        string_attr(&root.attributes, "cx.cli.query.language"),
        Some("promql")
    );
    assert_eq!(
        string_attr(&root.attributes, "gen_ai.agent.name"),
        Some("claude-code")
    );
    assert!(attr(&root.attributes, "cli.ci.provider").is_none());

    wait_until(|| directory.join(RETRY_AFTER).is_file()).await;
    wait_for_idle(&directory).await;
    assert_eq!(requests(&collector).await.len(), 3);
    // A new invocation can queue while the route is cooling down.
    assert_success(
        home.background(&["--version"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    assert_eq!(batches(&directory).len(), 2);
    assert_success(
        home.background(&["__telemetry-send"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    assert_eq!(requests(&collector).await.len(), 3);

    online.store(true, Ordering::SeqCst);
    fs::remove_file(directory.join(RETRY_AFTER)).unwrap();
    let helper = home
        .background(&["__telemetry-send"], &endpoint)
        .output()
        .await
        .unwrap();
    assert!(helper.stdout.is_empty() && helper.stderr.is_empty());
    assert_success(helper);
    let got = requests(&collector).await;
    assert_eq!(got.len(), 4);
    assert_eq!(got[3].body, queued);
    assert_eq!(
        got[3].headers["authorization"],
        format!("Bearer {COLLECTOR_KEY}")
    );
    assert_eq!(got[3].headers["content-type"], "application/x-protobuf");
    assert_eq!(batches(&directory).len(), 1);
    assert_success(
        home.background(&["__telemetry-send"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    assert_eq!(requests(&collector).await.len(), 4);

    // The sender preserves the service version stored by an earlier cx build.
    let path = batches(&directory).pop().unwrap();
    let mut older = ExportTraceServiceRequest::decode(fs::read(&path).unwrap().as_slice()).unwrap();
    let version = older.resource_spans[0]
        .resource
        .as_mut()
        .unwrap()
        .attributes
        .iter_mut()
        .find(|attr| attr.key == "service.version")
        .unwrap();
    version.value = Some(AnyValue {
        value: Some(Value::StringValue("0.1.0".into())),
    });
    let older_bytes = older.encode_to_vec();
    fs::write(path, &older_bytes).unwrap();
    home.config(b"telemetry = true\n");
    assert_success(
        home.background(&["__telemetry-send"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    let got = requests(&collector).await;
    assert_eq!(got.len(), 5);
    assert_eq!(got[4].body, older_bytes);
    assert!(batches(&directory).is_empty());
}

#[tokio::test]
async fn background_delivery_drops_running_command_when_consent_is_revoked_before_shutdown() {
    let home = Sandbox::new();
    let api = MockServer::start().await;
    home.profile(&api.uri());
    home.config(b"telemetry = true\n");
    let (arrived_tx, arrived_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(Some(release_rx)));
    Mock::given(method("GET"))
        .and(path("/metrics/api/v1/query"))
        .respond_with({
            let release_rx = Arc::clone(&release_rx);
            move |_: &Request| {
                let _ = arrived_tx.send(());
                if let Some(receiver) = release_rx.lock().unwrap().as_ref() {
                    let _ = receiver.recv_timeout(Duration::from_secs(5));
                }
                ResponseTemplate::new(200).set_body_json(json!({
                    "status": "success",
                    "data": { "resultType": "vector", "result": [] }
                }))
            }
        })
        .mount(&api)
        .await;
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&collector)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", collector.uri());

    let child = home
        .background(
            &[
                "metrics",
                "query",
                "up",
                "-p",
                PROFILE,
                "--no-console-link",
                "-o",
                "json",
            ],
            &endpoint,
        )
        .spawn()
        .unwrap();
    arrived_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    home.config(b"telemetry = false\n");
    release_tx.send(()).unwrap();
    assert_success(child.wait_with_output().await.unwrap());
    assert!(requests(&collector).await.is_empty());
    let telemetry = home.cx_dir().join("telemetry");
    assert!(!telemetry.exists());

    home.config(b"telemetry = true\n");
    assert_success(
        home.background(&["__telemetry-send"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    assert!(requests(&collector).await.is_empty());
}

#[tokio::test]
async fn detached_sender_does_not_block_commands_or_duplicate_concurrent_delivery() {
    let home = Sandbox::new();
    let collector = MockServer::start().await;
    let posts = Arc::new(AtomicUsize::new(0));
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with({
            let posts = Arc::clone(&posts);
            move |_: &Request| {
                if posts.fetch_add(1, Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(200).set_delay(Duration::from_secs(10))
                } else {
                    ResponseTemplate::new(200)
                }
            }
        })
        .mount(&collector)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", collector.uri());
    let started = Instant::now();
    assert_success(
        home.background(&["--version"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    assert!(started.elapsed() < Duration::from_millis(2500));
    let directory = home.partition();
    wait_for_requests(&collector, 1).await;
    assert!(Spool::new(directory.clone()).try_lock().is_err());

    let started = Instant::now();
    assert_success(
        home.background(&["__telemetry-send"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    assert!(started.elapsed() < Duration::from_millis(2500));
    assert_eq!(requests(&collector).await.len(), 1);
    assert_eq!(batches(&directory).len(), 1);

    let got = wait_for_requests(&collector, 2).await;
    assert_eq!(got[0].body, got[1].body);
    wait_until(|| batches(&directory).is_empty()).await;
    wait_for_idle(&directory).await;
    assert_eq!(requests(&collector).await.len(), 2);
}

#[tokio::test]
async fn a_disconnected_collector_can_receive_the_original_trace_after_recovery() {
    let home = Sandbox::new();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let endpoint = format!("http://{}{TRACE_PATH}", listener.local_addr().unwrap());
    let (recover, mut recovering) = tokio::sync::oneshot::channel();
    let unavailable = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut recovering => return listener,
                accepted = listener.accept() => drop(accepted.unwrap()),
            }
        }
    });

    assert_success(
        home.background(&["--version"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    let directory = home.partition();
    wait_until(|| directory.join(RETRY_AFTER).is_file()).await;
    wait_for_idle(&directory).await;
    let queued = fs::read(batches(&directory).pop().unwrap()).unwrap();

    recover.send(()).unwrap();
    let listener = unavailable.await.unwrap().into_std().unwrap();
    let collector = MockServer::builder().listener(listener).start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&collector)
        .await;
    fs::write(directory.join(RETRY_AFTER), "0").unwrap();
    assert_success(
        home.background(&["__telemetry-send"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    let received = requests(&collector).await;
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].body, queued);
    assert!(batches(&directory).is_empty());
}

#[tokio::test]
async fn edited_spool_with_unknown_unsafe_or_missing_fields_is_discarded() {
    let home = Sandbox::new();
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(503))
        .mount(&collector)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", collector.uri());
    assert_success(
        home.background(&["--version"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    let directory = home.partition();
    wait_until(|| directory.join(RETRY_AFTER).is_file()).await;
    wait_for_idle(&directory).await;
    for key in [
        "process.command_args",
        "cx.cli.query.language",
        "process.exit.code",
    ] {
        assert_success(
            home.background(&["--version"], &endpoint)
                .output()
                .await
                .unwrap(),
        );
        let path = batches(&directory).pop().unwrap();
        let mut request =
            ExportTraceServiceRequest::decode(fs::read(&path).unwrap().as_slice()).unwrap();
        let attributes = &mut request.resource_spans[0].scope_spans[0].spans[0].attributes;
        if key == "process.exit.code" {
            attributes.retain(|attribute| attribute.key != key);
        } else {
            attributes.push(KeyValue {
                key: key.to_owned(),
                value: Some(AnyValue {
                    value: Some(Value::StringValue("edited-spool-secret".into())),
                }),
                ..Default::default()
            });
        }
        fs::write(path, request.encode_to_vec()).unwrap();
    }
    fs::remove_file(directory.join(RETRY_AFTER)).unwrap();
    collector.reset().await;
    assert_success(
        home.background(&["__telemetry-send"], &endpoint)
            .output()
            .await
            .unwrap(),
    );
    let sent = requests(&collector).await;
    assert_eq!(sent.len(), 1);
    assert!(!sent[0]
        .body
        .windows("edited-spool-secret".len())
        .any(|bytes| bytes == b"edited-spool-secret"));
    assert!(batches(&directory).is_empty());
}

#[tokio::test]
async fn background_queues_are_isolated_by_explicit_destination() {
    let home = Sandbox::new();
    let first = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(503))
        .mount(&first)
        .await;
    let first_endpoint = format!("{}{TRACE_PATH}", first.uri());
    assert_success(
        home.background(&["--version"], &first_endpoint)
            .env(
                "TRACEPARENT",
                "00-11111111111111111111111111111111-2222222222222222-01",
            )
            .output()
            .await
            .unwrap(),
    );
    let first_partition = home.partition();
    let original = fs::read(batches(&first_partition).pop().unwrap()).unwrap();

    let second = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&second)
        .await;
    let second_endpoint = format!("{}{TRACE_PATH}", second.uri());
    let mut command = home.cx(&["--version"]);
    command
        .env("CX_TELEMETRY_ENDPOINT", &second_endpoint)
        .env("CX_TELEMETRY_DELIVERY", "background")
        .env(
            "CX_TELEMETRY_KEY_URL",
            format!("{}{KEY_PATH}", second.uri()),
        )
        .env("CX_API_KEY", PROFILE_KEY)
        .env(
            "TRACEPARENT",
            "00-33333333333333333333333333333333-4444444444444444-01",
        );
    assert_success(command.output().await.unwrap());
    let received = wait_for_requests(&second, 1).await;
    assert_eq!(received[0].url.path(), TRACE_PATH);
    assert!(!received[0].headers.contains_key("authorization"));
    assert_eq!(
        spans(&decode(&received[0].body)).next().unwrap().trace_id,
        [0x33; 16]
    );
    assert_eq!(
        fs::read(batches(&first_partition).pop().unwrap()).unwrap(),
        original
    );
    wait_until(|| first_partition.join(RETRY_AFTER).is_file()).await;
    wait_for_idle(&first_partition).await;
    assert_eq!(requests(&second).await.len(), 1);
}

#[tokio::test]
async fn discovery_caches_expires_and_refreshes_keys_after_auth_rejection() {
    if !cfg!(debug_assertions) {
        return;
    }
    let home = Sandbox::new();
    let ingress = MockServer::start().await;
    let key_server = MockServer::start().await;
    let generation = Arc::new(AtomicUsize::new(0));
    let fetches = Arc::new(AtomicUsize::new(0));
    let keys = [
        "telemetry-key-old",
        "telemetry-key-rotated",
        "telemetry-key-again",
    ];
    let host_url = ingress.uri();
    Mock::given(method("GET"))
        .and(path(KEY_PATH))
        .respond_with({
            let generation = Arc::clone(&generation);
            move |_: &Request| {
                if fetches.fetch_add(1, Ordering::SeqCst) == 0 {
                    return ResponseTemplate::new(503);
                }
                let index = generation.load(Ordering::SeqCst).min(keys.len() - 1);
                let key = keys[index];
                ResponseTemplate::new(200).set_body_json(json!({"key": key, "host_url": host_url}))
            }
        })
        .mount(&key_server)
        .await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with({
            let generation = Arc::clone(&generation);
            move |request: &Request| {
                let index = generation.load(Ordering::SeqCst);
                if keys
                    .get(index)
                    .is_some_and(|key| request.headers["authorization"] == format!("Bearer {key}"))
                {
                    ResponseTemplate::new(200)
                } else {
                    ResponseTemplate::new(if index == 1 { 401 } else { 403 })
                }
            }
        })
        .mount(&ingress)
        .await;
    let key_url = format!("{}{KEY_PATH}", key_server.uri());

    assert_success(
        home.discovery(&["--version"], &key_url)
            .output()
            .await
            .unwrap(),
    );
    let directory = home.partition();
    let cache = directory.join(KEY_CACHE);
    let first = wait_for_requests(&ingress, 1).await;
    wait_until(|| batches(&directory).is_empty()).await;
    wait_for_idle(&directory).await;
    assert_eq!(requests(&key_server).await.len(), 2);
    assert_eq!(
        first[0].headers["authorization"],
        format!("Bearer {}", keys[0])
    );
    let data = decode(&first[0].body);
    let attrs = &data.resource.unwrap().attributes;
    assert_eq!(string_attr(attrs, "cx.application.name"), Some("cx-cli"));
    assert_eq!(string_attr(attrs, "cx.subsystem.name"), Some("usage"));
    assert_private(&first[0].body);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    assert_success(
        home.discovery(&["--version"], &key_url)
            .output()
            .await
            .unwrap(),
    );
    wait_for_requests(&ingress, 2).await;
    wait_until(|| batches(&directory).is_empty()).await;
    wait_for_idle(&directory).await;
    assert_eq!(requests(&key_server).await.len(), 2);

    let mut expired: serde_json::Value =
        serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
    expired["fetched_at"] = json!(0);
    fs::write(&cache, serde_json::to_vec(&expired).unwrap()).unwrap();
    assert_success(
        home.discovery(&["--version"], &key_url)
            .output()
            .await
            .unwrap(),
    );
    wait_for_requests(&ingress, 3).await;
    wait_until(|| batches(&directory).is_empty()).await;
    wait_for_idle(&directory).await;
    assert_eq!(requests(&key_server).await.len(), 3);

    for index in 1..=2 {
        generation.store(index, Ordering::SeqCst);
        assert_success(
            home.discovery(&["--version"], &key_url)
                .output()
                .await
                .unwrap(),
        );
        let got = wait_for_requests(&ingress, 3 + 2 * index).await;
        wait_until(|| batches(&directory).is_empty()).await;
        wait_for_idle(&directory).await;
        assert_eq!(requests(&key_server).await.len(), 3 + index);
        let (rejected, accepted) = (&got[got.len() - 2], &got[got.len() - 1]);
        assert_eq!(
            rejected.headers["authorization"],
            format!("Bearer {}", keys[index - 1])
        );
        assert_eq!(
            accepted.headers["authorization"],
            format!("Bearer {}", keys[index])
        );
        assert_eq!(rejected.body, accepted.body);
        assert_private(&accepted.body);
        assert!(!accepted
            .body
            .windows(keys[index].len())
            .any(|value| value == keys[index].as_bytes()));
    }
    generation.store(keys.len(), Ordering::SeqCst);
    assert_success(
        home.discovery(&["--version"], &key_url)
            .output()
            .await
            .unwrap(),
    );
    let got = wait_for_requests(&ingress, 9).await;
    wait_until(|| directory.join(RETRY_AFTER).is_file()).await;
    wait_for_idle(&directory).await;
    let pending = batches(&directory);
    assert_eq!(pending.len(), 1);
    assert_eq!(fs::read(&pending[0]).unwrap(), got[7].body);
    assert_eq!(got[7].body, got[8].body);
    assert_eq!(requests(&key_server).await.len(), 6);
    assert!(!cache.exists());
    assert!(requests(&key_server)
        .await
        .iter()
        .all(|request| !request.headers.contains_key("authorization")));
}

#[tokio::test]
async fn discovery_rejects_unsafe_hosts_and_never_follows_redirects() {
    if !cfg!(debug_assertions) {
        return;
    }
    let wrong = MockServer::start().await;
    for case in ["non-bare ingress", "redirected key", "redirected ingress"] {
        let home = Sandbox::new();
        let keys = MockServer::start().await;
        let ingress = MockServer::start().await;
        let response = match case {
            "non-bare ingress" => ResponseTemplate::new(200).set_body_json(json!({
                "key": COLLECTOR_KEY,
                "host_url": format!("{}/hidden/..", wrong.uri())
            })),
            "redirected key" => ResponseTemplate::new(307).insert_header("location", wrong.uri()),
            _ => ResponseTemplate::new(200)
                .set_body_json(json!({"key": COLLECTOR_KEY, "host_url": ingress.uri()})),
        };
        Mock::given(method("GET"))
            .and(path(KEY_PATH))
            .respond_with(response)
            .mount(&keys)
            .await;
        Mock::given(method("POST"))
            .and(path(TRACE_PATH))
            .respond_with(ResponseTemplate::new(307).insert_header("location", wrong.uri()))
            .mount(&ingress)
            .await;
        let key_url = format!("{}{KEY_PATH}", keys.uri());
        assert_success(
            home.discovery(&["--version"], &key_url)
                .output()
                .await
                .unwrap(),
        );
        let directory = home.partition();
        if case == "redirected ingress" {
            wait_for_requests(&ingress, 1).await;
            wait_until(|| batches(&directory).is_empty()).await;
            assert!(directory.join(KEY_CACHE).is_file());
        } else {
            wait_until(|| directory.join(RETRY_AFTER).is_file()).await;
            assert_eq!(batches(&directory).len(), 1);
            assert!(!directory.join(KEY_CACHE).exists());
            assert!(requests(&ingress).await.is_empty());
        }
        wait_for_idle(&directory).await;
        assert_eq!(requests(&keys).await.len(), 1);
        assert!(requests(&wrong).await.is_empty());
    }
}

#[tokio::test]
async fn release_default_notice_prints_once_before_collection() {
    let home = Sandbox::new();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ingress)
        .await;
    let keys = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(KEY_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"key": COLLECTOR_KEY, "host_url": ingress.uri()})),
        )
        .mount(&keys)
        .await;
    let key_url = format!("{}{KEY_PATH}", keys.uri());

    let first = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_KEY_URL", &key_url)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(first.status.success(), "cx exited with {}", first.status);
    let first_stderr = String::from_utf8_lossy(&first.stderr);
    assert!(first_stderr.contains(RELEASE_NOTICE));
    wait_for_requests(&ingress, 1).await;

    let second = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_KEY_URL", &key_url)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(second.status.success(), "cx exited with {}", second.status);
    let second_stderr = String::from_utf8_lossy(&second.stderr);
    assert!(!second_stderr.contains(RELEASE_NOTICE));
    wait_for_requests(&ingress, 2).await;
}

#[tokio::test]
async fn release_default_notice_is_suppressed_when_opted_out() {
    let home = Sandbox::new();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ingress)
        .await;
    let keys = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(KEY_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"key": COLLECTOR_KEY, "host_url": ingress.uri()})),
        )
        .mount(&keys)
        .await;
    let key_url = format!("{}{KEY_PATH}", keys.uri());

    for (variable, value) in [("CX_TELEMETRY", "off"), ("DO_NOT_TRACK", "1")] {
        let output = home
            .cx(&["--version"])
            .env("CX_TELEMETRY_KEY_URL", &key_url)
            .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
            .env(variable, value)
            .output()
            .await
            .unwrap();
        assert!(output.status.success(), "cx exited with {}", output.status);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains(RELEASE_NOTICE), "{variable}={value}");
    }
    assert!(requests(&keys).await.is_empty());
    assert!(requests(&ingress).await.is_empty());
}

#[tokio::test]
async fn release_default_notice_repeats_when_marker_persistence_is_unavailable() {
    let home = Sandbox::new();
    fs::create_dir_all(home.notice_dir().parent().unwrap()).unwrap();
    fs::write(home.notice_dir(), "not-a-directory").unwrap();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ingress)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", ingress.uri());

    let first = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(first.status.success(), "cx exited with {}", first.status);
    assert!(String::from_utf8_lossy(&first.stderr).contains(RELEASE_NOTICE));

    let second = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(second.status.success(), "cx exited with {}", second.status);
    assert!(String::from_utf8_lossy(&second.stderr).contains(RELEASE_NOTICE));
    wait_for_requests(&ingress, 2).await;
}

#[tokio::test]
async fn release_default_notice_write_failure_disables_collection() {
    if !cfg!(debug_assertions) {
        return;
    }
    let home = Sandbox::new();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ingress)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", ingress.uri());

    let output = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .env("CX_TELEMETRY_TEST_FAIL_NOTICE_STDERR", "1")
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "cx exited with {}", output.status);
    assert!(requests(&ingress).await.is_empty());
    assert!(!home.cx_dir().join("state.json").exists());
    assert!(!home.notice_marker().exists());
}

#[tokio::test]
async fn config_write_after_notice_failure_does_not_persist_explicit_telemetry_true() {
    if !cfg!(debug_assertions) {
        return;
    }
    let home = Sandbox::new();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ingress)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", ingress.uri());
    home.profile(&ingress.uri());

    let failed_notice = home
        .cx(&["profiles", "set-default", PROFILE])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .env("CX_TELEMETRY_TEST_FAIL_NOTICE_STDERR", "1")
        .output()
        .await
        .unwrap();
    assert!(
        failed_notice.status.success(),
        "cx exited with {}",
        failed_notice.status
    );
    assert!(requests(&ingress).await.is_empty());
    assert!(!home.notice_marker().exists());
    let saved = fs::read_to_string(home.cx_dir().join("config.toml"))
        .unwrap()
        .parse::<toml::Table>()
        .unwrap();
    assert_eq!(saved["default_profile"].as_str(), Some(PROFILE));
    assert!(
        !saved.contains_key("telemetry"),
        "telemetry=true must not be serialized as explicit consent"
    );

    let disclosed = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(
        disclosed.status.success(),
        "cx exited with {}",
        disclosed.status
    );
    assert!(String::from_utf8_lossy(&disclosed.stderr).contains(RELEASE_NOTICE));
    wait_for_requests(&ingress, 1).await;

    let subsequent = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(
        subsequent.status.success(),
        "cx exited with {}",
        subsequent.status
    );
    assert!(!String::from_utf8_lossy(&subsequent.stderr).contains(RELEASE_NOTICE));
    wait_for_requests(&ingress, 2).await;
}

#[tokio::test]
async fn release_default_notice_ignores_invalid_marker_directory() {
    let home = Sandbox::new();
    fs::create_dir_all(home.notice_dir()).unwrap();
    fs::create_dir_all(home.notice_marker()).unwrap();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ingress)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", ingress.uri());

    let output = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "cx exited with {}", output.status);
    assert!(String::from_utf8_lossy(&output.stderr).contains(RELEASE_NOTICE));
    wait_for_requests(&ingress, 1).await;
}

#[tokio::test]
async fn release_default_notice_ignores_empty_marker_file() {
    let home = Sandbox::new();
    fs::create_dir_all(home.notice_dir()).unwrap();
    fs::write(home.notice_marker(), []).unwrap();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ingress)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", ingress.uri());

    let first = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(first.status.success(), "cx exited with {}", first.status);
    assert!(String::from_utf8_lossy(&first.stderr).contains(RELEASE_NOTICE));

    let second = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(second.status.success(), "cx exited with {}", second.status);
    assert!(!String::from_utf8_lossy(&second.stderr).contains(RELEASE_NOTICE));
}

#[tokio::test]
async fn release_default_notice_retries_after_stderr_failure() {
    if !cfg!(debug_assertions) {
        return;
    }
    let home = Sandbox::new();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ingress)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", ingress.uri());

    let failed = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .env("CX_TELEMETRY_TEST_FAIL_NOTICE_STDERR", "1")
        .output()
        .await
        .unwrap();
    assert!(failed.status.success(), "cx exited with {}", failed.status);
    assert!(requests(&ingress).await.is_empty());
    assert!(!home.notice_marker().exists());

    let first_success = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(
        first_success.status.success(),
        "cx exited with {}",
        first_success.status
    );
    assert!(String::from_utf8_lossy(&first_success.stderr).contains(RELEASE_NOTICE));

    let second_success = home
        .cx(&["--version"])
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&second_success.stderr).contains(RELEASE_NOTICE),
        "completed marker should suppress repeated notice"
    );
}

#[tokio::test]
async fn concurrent_first_run_claims_notice_marker_once() {
    if !cfg!(debug_assertions) {
        return;
    }
    let home = Sandbox::new();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ingress)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", ingress.uri());

    let mut first = home.cx(&["--version"]);
    first
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1");
    let mut second = home.cx(&["--version"]);
    second
        .env("CX_TELEMETRY_ENDPOINT", &endpoint)
        .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1");

    let (first, second) = tokio::join!(first.output(), second.output());
    let first = first.unwrap();
    let second = second.unwrap();
    assert!(first.status.success());
    assert!(second.status.success());
    let notices = [first, second]
        .iter()
        .filter(|output| String::from_utf8_lossy(&output.stderr).contains(RELEASE_NOTICE))
        .count();
    assert_eq!(notices, 1);
}

#[tokio::test]
async fn sender_never_prints_release_default_notice() {
    if !cfg!(debug_assertions) {
        return;
    }
    let home = Sandbox::new();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(503))
        .mount(&ingress)
        .await;
    let keys = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(KEY_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"key": COLLECTOR_KEY, "host_url": ingress.uri()})),
        )
        .mount(&keys)
        .await;
    let key_url = format!("{}{KEY_PATH}", keys.uri());
    assert_success(
        home.cx(&["--version"])
            .env("CX_TELEMETRY_KEY_URL", &key_url)
            .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
            .output()
            .await
            .unwrap(),
    );
    let telemetry_directory = home.cx_dir().join("telemetry");
    wait_until(|| telemetry_directory.is_dir()).await;
    let partition = home.partition();
    wait_until(|| partition.join(RETRY_AFTER).is_file()).await;
    wait_for_idle(&partition).await;

    let sender = home
        .cx(&["__telemetry-send"])
        .env("CX_TELEMETRY_KEY_URL", &key_url)
        .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
        .output()
        .await
        .unwrap();
    assert!(sender.status.success(), "cx exited with {}", sender.status);
    assert!(sender.stderr.is_empty());
    assert!(sender.stdout.is_empty());
}
