use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cli_otel::Spool;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::logs::v1::SeverityNumber;
use opentelemetry_proto::tonic::trace::v1::span::SpanKind;
use prost::Message;
use serde_json::json;
use tokio::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const TRACES: &str = "/v1/traces";
const LOGS: &str = "/v1/logs";
const PRIVATE_ID: &str = "failure-logs-private-view-id";
const PRIVATE_ERROR: &str = "failure-logs-private-server-error";
const CUSTOMER_KEY: &str = "failure-logs-private-customer-key";
const COLLECTOR_KEY: &str = "failure-logs-private-collector-key";

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("cx-failure-logs-{}", uuid::Uuid::new_v4())))
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

    fn explicit(&self, args: &[&str], endpoint: &str) -> Command {
        let mut command = self.cx(args);
        command
            .env("CX_TELEMETRY_ENDPOINT", endpoint)
            .env("CX_TELEMETRY_FAILURE_LOGS", "1")
            .env("CX_TELEMETRY_API_KEY", COLLECTOR_KEY);
        command
    }

    fn config_path(&self) -> PathBuf {
        self.0.join(".cx/config.toml")
    }

    fn queue(&self) -> PathBuf {
        fs::read_dir(self.0.join(".cx/telemetry"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn queue_files(queue: &Path) -> Vec<PathBuf> {
    fs::read_dir(queue)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "otlp")
        })
        .collect()
}

async fn wait_until(mut ready: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("sender did not reach the expected state");
}

async fn wait_for_requests(server: &MockServer, count: usize) -> Vec<Request> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let requests = server.received_requests().await.unwrap();
            if requests.len() >= count {
                return requests;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("collector did not receive the expected telemetry")
}

fn assert_correlated(
    traces: &[u8],
    logs: &[u8],
    expected_error: &str,
    expected_exit: i64,
    expected_args: &[&str],
) {
    let traces = ExportTraceServiceRequest::decode(traces).unwrap();
    let logs = ExportLogsServiceRequest::decode(logs).unwrap();
    let [log_resource] = logs.resource_logs.as_slice() else {
        panic!("expected a single resource");
    };
    assert_eq!(log_resource.resource, traces.resource_spans[0].resource);
    let [log_scope] = log_resource.scope_logs.as_slice() else {
        panic!("expected a single scope");
    };
    let trace_scope = &traces.resource_spans[0].scope_spans[0];
    assert_eq!(log_scope.scope, trace_scope.scope);
    let [log] = log_scope.log_records.as_slice() else {
        panic!("expected one record for the failed command");
    };
    let root = trace_scope
        .spans
        .iter()
        .find(|span| span.kind == SpanKind::Internal as i32)
        .unwrap();
    assert_eq!(log.trace_id, root.trace_id);
    assert_eq!(log.span_id, root.span_id);
    assert_eq!(log.time_unix_nano, root.end_time_unix_nano);
    assert_eq!(log.flags, root.flags & 0xff);
    assert_eq!(log.severity_number, SeverityNumber::Error as i32);
    assert_eq!(log.severity_text, "ERROR");
    assert_eq!(
        log.body.as_ref().unwrap().value,
        Some(Value::StringValue("CLI command failed".into()))
    );

    let allowed = [
        "process.executable.name",
        "process.exit.code",
        "cli.termination.reason",
        "error.type",
        "process.command_args",
    ];
    let expected: Vec<_> = allowed
        .into_iter()
        .map(|key| root.attributes.iter().find(|attr| attr.key == key).unwrap())
        .collect();
    assert_eq!(log.attributes.iter().collect::<Vec<_>>(), expected);
    assert!(root
        .attributes
        .iter()
        .all(|attr| !matches!(attr.key.as_str(), "cli.command" | "cli.flags")));
    let Some(Value::ArrayValue(args)) = &log.attributes[4].value.as_ref().unwrap().value else {
        panic!("expected sanitized command arguments");
    };
    let args: Vec<_> = args
        .values
        .iter()
        .map(|item| match &item.value {
            Some(Value::StringValue(value)) => value.as_str(),
            _ => panic!("expected a string command argument"),
        })
        .collect();
    assert_eq!(args, expected_args);
    assert_eq!(
        log.attributes[1].value.as_ref().unwrap().value,
        Some(Value::IntValue(expected_exit))
    );
    assert_eq!(
        log.attributes[3].value.as_ref().unwrap().value,
        Some(Value::StringValue(expected_error.to_owned()))
    );
}

#[tokio::test]
async fn inline_logs_correlate_the_failed_command_and_exclude_private_diagnostics() {
    let home = Home::new();
    let collector = MockServer::start().await;
    let api = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/mgmt/openapi/5/data-exploration/views/v1/views/{PRIVATE_ID}"
        )))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({ "message": PRIVATE_ERROR })))
        .mount(&api)
        .await;
    for signal in [TRACES, LOGS] {
        Mock::given(method("POST"))
            .and(path(signal))
            .respond_with(ResponseTemplate::new(200))
            .mount(&collector)
            .await;
    }
    let endpoint = format!("{}{TRACES}", collector.uri());
    let failed = home
        .explicit(
            &["views", "get", PRIVATE_ID, "--no-console-link"],
            &endpoint,
        )
        .env("CX_REGION", api.uri())
        .env("CX_API_KEY", CUSTOMER_KEY)
        .env(
            "TRACEPARENT",
            "00-11111111111111111111111111111111-2222222222222222-01",
        )
        .output()
        .await
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains(PRIVATE_ERROR));

    let requests = collector.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let traces = requests
        .iter()
        .find(|request| request.url.path() == TRACES)
        .unwrap();
    let logs = requests
        .iter()
        .find(|request| request.url.path() == LOGS)
        .unwrap();
    assert_correlated(
        &traces.body,
        &logs.body,
        "permission",
        1,
        &["cx", "views", "get", "--no-console-link"],
    );
    for request in &requests {
        assert_eq!(
            request.headers["authorization"],
            format!("Bearer {COLLECTOR_KEY}")
        );
        assert_eq!(request.headers["content-type"], "application/x-protobuf");
        for secret in [PRIVATE_ID, PRIVATE_ERROR, CUSTOMER_KEY, COLLECTOR_KEY] {
            assert!(!request
                .body
                .windows(secret.len())
                .any(|bytes| bytes == secret.as_bytes()));
        }
    }

    let success = home
        .explicit(&["--version"], &endpoint)
        .output()
        .await
        .unwrap();
    assert!(success.status.success());
    let parse = home
        .explicit(&[PRIVATE_ID], &endpoint)
        .output()
        .await
        .unwrap();
    assert_eq!(parse.status.code(), Some(2));
    let requests = collector.received_requests().await.unwrap();
    assert_eq!(requests[2].url.path(), TRACES);
    assert_eq!(requests.len(), 5);
    let traces = requests[3..]
        .iter()
        .find(|request| request.url.path() == TRACES)
        .unwrap();
    let logs = requests[3..]
        .iter()
        .find(|request| request.url.path() == LOGS)
        .unwrap();
    assert_correlated(&traces.body, &logs.body, "argument_parse", 2, &["cx"]);
    assert!(!logs
        .body
        .windows(PRIVATE_ID.len())
        .any(|bytes| bytes == PRIVATE_ID.as_bytes()));
}

#[tokio::test]
async fn inline_failure_logs_share_a_deadline_and_require_consent_and_a_standard_route() {
    let home = Home::new();
    let collector = MockServer::start().await;
    for signal in [TRACES, LOGS] {
        Mock::given(method("POST"))
            .and(path(signal))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
            .mount(&collector)
            .await;
    }
    let endpoint = format!("{}{TRACES}", collector.uri());
    let started = Instant::now();
    let failure = home
        .explicit(&[PRIVATE_ID], &endpoint)
        .output()
        .await
        .unwrap();
    assert_eq!(failure.status.code(), Some(2));
    assert!(started.elapsed() < Duration::from_millis(1750));
    assert_eq!(collector.received_requests().await.unwrap().len(), 2);

    let opted_out = Home::new();
    let failure = opted_out
        .explicit(&[PRIVATE_ID], &endpoint)
        .env("DO_NOT_TRACK", "1")
        .output()
        .await
        .unwrap();
    assert_eq!(failure.status.code(), Some(2));
    assert!(!opted_out.0.join(".cx/state.json").exists());
    assert!(!opted_out.0.join(".cx/telemetry").exists());
    assert_eq!(collector.received_requests().await.unwrap().len(), 2);

    let custom = format!("{}/custom-traces?route=private", collector.uri());
    let failure = home
        .explicit(&[PRIVATE_ID], &custom)
        .output()
        .await
        .unwrap();
    assert_eq!(failure.status.code(), Some(2));
    let requests = collector.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[2].url.path(), "/custom-traces");
    assert_eq!(requests[2].url.query(), Some("route=private"));
}

#[tokio::test]
async fn background_failure_keeps_trace_batch_until_logs_arrive_and_rechecks_consent() {
    let home = Home::new();
    let collector = MockServer::start().await;
    let revoke_consent = Arc::new(AtomicBool::new(true));
    let logs_status = Arc::new(AtomicU16::new(503));
    Mock::given(method("POST"))
        .and(path(TRACES))
        .respond_with({
            let revoke_consent = Arc::clone(&revoke_consent);
            let config = home.config_path();
            move |_: &Request| {
                if revoke_consent.swap(false, Ordering::SeqCst) {
                    fs::write(&config, "telemetry = false\n").unwrap();
                }
                ResponseTemplate::new(200)
            }
        })
        .mount(&collector)
        .await;
    Mock::given(method("POST"))
        .and(path(LOGS))
        .respond_with({
            let status = Arc::clone(&logs_status);
            move |_: &Request| ResponseTemplate::new(status.load(Ordering::SeqCst))
        })
        .mount(&collector)
        .await;
    let endpoint = format!("{}{TRACES}", collector.uri());
    let background = |args: &[&str]| {
        let mut command = home.explicit(args, &endpoint);
        command.env("CX_TELEMETRY_DELIVERY", "background");
        command
    };
    let failure = background(&[PRIVATE_ID]).output().await.unwrap();
    assert_eq!(failure.status.code(), Some(2));
    let queue = home.queue();
    let spool = Spool::new(queue.clone());
    let got = wait_for_requests(&collector, 1).await;
    wait_until(|| spool.try_lock().is_ok()).await;
    assert_eq!(got[0].url.path(), TRACES);
    let pending = queue_files(&queue);
    assert_eq!(pending.len(), 1);
    let queued = fs::read(&pending[0]).unwrap();
    assert_eq!(queued, got[0].body);
    assert_eq!(collector.received_requests().await.unwrap().len(), 1);

    let helper = background(&["__telemetry-send"]).output().await.unwrap();
    assert!(helper.status.success());
    assert_eq!(collector.received_requests().await.unwrap().len(), 1);
    assert_eq!(queue_files(&queue), pending);

    fs::write(home.config_path(), "telemetry = true\n").unwrap();
    let helper = background(&["__telemetry-send"]).output().await.unwrap();
    assert!(helper.status.success());
    let got = collector.received_requests().await.unwrap();
    assert_eq!(got.len(), 5);
    assert_eq!(got[1].url.path(), TRACES);
    assert_eq!(got[1].body, queued);
    for logs in &got[2..] {
        assert_eq!(logs.url.path(), LOGS);
        assert_eq!(logs.body, got[2].body);
        assert_correlated(&queued, &logs.body, "argument_parse", 2, &["cx"]);
    }
    assert_eq!(queue_files(&queue), pending);

    logs_status.store(200, Ordering::SeqCst);
    fs::remove_file(queue.join("retry-after")).unwrap();
    let helper = background(&["__telemetry-send"]).output().await.unwrap();
    assert!(helper.status.success());
    let got = collector.received_requests().await.unwrap();
    assert_eq!(got.len(), 7);
    assert_eq!(got[5].url.path(), TRACES);
    assert_eq!(got[5].body, queued);
    assert_eq!(got[6].url.path(), LOGS);
    assert_eq!(got[6].body, got[2].body);
    assert!(queue_files(&queue).is_empty());

    logs_status.store(400, Ordering::SeqCst);
    let failure = background(&[PRIVATE_ID]).output().await.unwrap();
    assert_eq!(failure.status.code(), Some(2));
    let got = wait_for_requests(&collector, 9).await;
    wait_until(|| spool.try_lock().is_ok()).await;
    assert_eq!(got[7].url.path(), TRACES);
    assert_eq!(got[8].url.path(), LOGS);
    assert!(queue_files(&queue).is_empty());
    assert!(!queue.join("retry-after").exists());
}

#[tokio::test]
async fn discovered_logs_refresh_the_key_and_replay_traces_when_ingress_changes() {
    if !cfg!(debug_assertions) {
        return;
    }
    let home = Home::new();
    let old_ingress = MockServer::start().await;
    let new_ingress = MockServer::start().await;
    let keys = MockServer::start().await;
    let rotated = Arc::new(AtomicBool::new(false));
    let old_key = "initial-failure-log-key";
    let new_key = "rotated-failure-log-key";
    Mock::given(method("GET"))
        .and(path("/key"))
        .respond_with({
            let rotated = Arc::clone(&rotated);
            let old_url = old_ingress.uri();
            let new_url = new_ingress.uri();
            move |_: &Request| {
                let (key, host_url) = if rotated.load(Ordering::SeqCst) {
                    (new_key, &new_url)
                } else {
                    (old_key, &old_url)
                };
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "key": key, "host_url": host_url }))
            }
        })
        .mount(&keys)
        .await;
    Mock::given(method("POST"))
        .and(path(TRACES))
        .respond_with(ResponseTemplate::new(200))
        .mount(&old_ingress)
        .await;
    Mock::given(method("POST"))
        .and(path(LOGS))
        .respond_with(move |_: &Request| {
            rotated.store(true, Ordering::SeqCst);
            ResponseTemplate::new(401)
        })
        .mount(&old_ingress)
        .await;
    for signal in [TRACES, LOGS] {
        Mock::given(method("POST"))
            .and(path(signal))
            .respond_with(ResponseTemplate::new(200))
            .mount(&new_ingress)
            .await;
    }
    let discover = |args: &[&str]| {
        let mut command = home.cx(args);
        command.env("CX_TELEMETRY_KEY_URL", format!("{}/key", keys.uri()));
        command
    };
    let failure = discover(&[PRIVATE_ID]).output().await.unwrap();
    assert_eq!(failure.status.code(), Some(2));
    let queue = home.queue();
    let cooldown = queue.join("retry-after");
    wait_until(|| cooldown.is_file()).await;
    let spool = Spool::new(queue.clone());
    wait_until(|| spool.try_lock().is_ok()).await;

    let original = old_ingress.received_requests().await.unwrap();
    assert_eq!(original.len(), 2);
    assert_eq!(original[0].url.path(), TRACES);
    assert_eq!(original[1].url.path(), LOGS);
    assert_eq!(
        original[1].headers["authorization"],
        format!("Bearer {old_key}")
    );
    assert!(new_ingress.received_requests().await.unwrap().is_empty());
    assert_eq!(queue_files(&queue).len(), 1);

    fs::remove_file(cooldown).unwrap();
    let helper = discover(&["__telemetry-send"]).output().await.unwrap();
    assert!(helper.status.success());
    let replay = new_ingress.received_requests().await.unwrap();
    assert_eq!(replay.len(), 2);
    assert_eq!(replay[0].url.path(), TRACES);
    assert_eq!(replay[1].url.path(), LOGS);
    for (replayed, previous) in replay.iter().zip(&original) {
        assert_eq!(
            replayed.headers["authorization"],
            format!("Bearer {new_key}")
        );
        assert_eq!(replayed.body, previous.body);
    }
    assert_correlated(
        &replay[0].body,
        &replay[1].body,
        "argument_parse",
        2,
        &["cx"],
    );
    assert!(queue_files(&queue).is_empty());
    let key_requests = keys.received_requests().await.unwrap();
    assert_eq!(key_requests.len(), 2);
    assert!(key_requests
        .iter()
        .all(|request| !request.headers.contains_key("authorization")));
}
