use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cli_otel::Spool;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use prost::Message;
use tokio::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const PRIVATE_INPUT: &str = "upstream-exporter-private-argument";
const PRIVATE_ENV: &str = "upstream-exporter-ambient-secret";
const TRACE_PATH: &str = "/v1/traces";
const LOG_PATH: &str = "/v1/logs";

struct Home(PathBuf);

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Home {
    fn cx(&self, arg: &str) -> Command {
        let mut cx = Command::new(env!("CARGO_BIN_EXE_cx"));
        cx.env_clear()
            .env("CX_HOME", &self.0)
            .env("CX_NO_UPDATE_NOTIFIER", "1")
            .env("CX_NO_SKILLS_AUTO_UPDATE", "1")
            .kill_on_drop(true)
            .arg(arg);
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            cx.env("SystemRoot", system_root);
        }
        cx
    }

    fn configured(&self, arg: &str, endpoint: &str, untrusted: &str) -> Command {
        let mut cx = self.cx(arg);
        cx.env("CX_TELEMETRY_ENDPOINT", endpoint)
            .env("CX_TELEMETRY_API_KEY", "approved-key")
            .env("CX_TELEMETRY_FAILURE_LOGS", "1")
            .env(
                "TRACEPARENT",
                "00-11111111111111111111111111111111-2222222222222222-01",
            )
            .env("OTEL_SDK_DISABLED", "true")
            .env("OTEL_SERVICE_NAME", PRIVATE_ENV)
            .env("OTEL_RESOURCE_ATTRIBUTES", format!("private={PRIVATE_ENV}"))
            .env("OTEL_EXPORTER_OTLP_ENDPOINT", untrusted)
            .env("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", untrusted)
            .env("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT", untrusted)
            .env("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc")
            .env("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "http/json")
            .env("OTEL_EXPORTER_OTLP_COMPRESSION", "bad-generic-compression")
            .env("OTEL_EXPORTER_OTLP_TRACES_TIMEOUT", "0")
            .env(
                "OTEL_EXPORTER_OTLP_HEADERS",
                format!("authorization={PRIVATE_ENV}"),
            )
            .env(
                "OTEL_EXPORTER_OTLP_TRACES_HEADERS",
                format!("x-hostile={PRIVATE_ENV},content-encoding=bad"),
            )
            .env(
                "OTEL_EXPORTER_OTLP_LOGS_HEADERS",
                format!("x-hostile={PRIVATE_ENV}"),
            )
            .env("OTEL_EXPORTER_OTLP_CERTIFICATE", "/missing/ca.pem")
            .env(
                "OTEL_EXPORTER_OTLP_TRACES_CERTIFICATE",
                "/missing/traces-ca.pem",
            )
            .env(
                "OTEL_EXPORTER_OTLP_TRACES_CLIENT_KEY",
                "/missing/client-key.pem",
            )
            .env("SSL_CERT_FILE", "/missing/system-ca.pem");
        cx
    }
}

fn assert_private_signals(trace: &Request, log: &Request) {
    for request in [trace, log] {
        assert_eq!(request.headers["authorization"], "Bearer approved-key");
        assert_eq!(request.headers["content-type"], "application/x-protobuf");
        assert!(!request.headers.contains_key("content-encoding"));
        assert!(!request.headers.contains_key("x-hostile"));
        for secret in [
            PRIVATE_ENV,
            PRIVATE_INPUT,
            "approved-key",
            "bad-generic-compression",
        ] {
            assert!(!request
                .body
                .windows(secret.len())
                .any(|bytes| bytes == secret.as_bytes()));
        }
    }
    let traces = ExportTraceServiceRequest::decode(trace.body.as_slice()).unwrap();
    let resource = &traces.resource_spans[0];
    let scope = &resource.scope_spans[0];
    let root = &scope.spans[0];
    assert_eq!(root.trace_id, [0x11; 16]);
    assert_eq!(root.parent_span_id, [0x22; 8]);
    let logs = ExportLogsServiceRequest::decode(log.body.as_slice()).unwrap();
    assert_eq!(logs.resource_logs[0].resource, resource.resource);
    let log_scope = &logs.resource_logs[0].scope_logs[0];
    assert_eq!(log_scope.scope, scope.scope);
    assert_eq!(log_scope.log_records.len(), 1);
    let log = &log_scope.log_records[0];
    assert_eq!(root.trace_id, log.trace_id);
    assert_eq!(root.span_id, log.span_id);
    assert_eq!(root.end_time_unix_nano, log.time_unix_nano);
}

#[tokio::test]
async fn upstream_exporter_uses_explicit_transport_for_inline_delivery_and_queue_replay() {
    let home =
        Home(std::env::temp_dir().join(format!("cx-otel-exporter-{}", uuid::Uuid::new_v4())));
    let collector = MockServer::start().await;
    let untrusted = MockServer::start().await;
    let available = Arc::new(AtomicBool::new(true));
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with({
            let available = Arc::clone(&available);
            move |_: &Request| {
                ResponseTemplate::new(if available.load(Ordering::SeqCst) {
                    200
                } else {
                    503
                })
            }
        })
        .mount(&collector)
        .await;
    Mock::given(method("POST"))
        .and(path(LOG_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&collector)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", collector.uri());
    let untrusted_endpoint = untrusted.uri();
    let baseline = home
        .cx(PRIVATE_INPUT)
        .env("CX_TELEMETRY", "0")
        .output()
        .await
        .unwrap();
    let inline = home
        .configured(PRIVATE_INPUT, &endpoint, &untrusted_endpoint)
        .env(
            "OTEL_EXPORTER_OTLP_TRACES_COMPRESSION",
            "bad-traces-compression",
        )
        .output()
        .await
        .unwrap();
    assert_eq!(inline.status.code(), Some(2));
    assert_eq!(inline.stdout, baseline.stdout);
    assert_eq!(inline.stderr, baseline.stderr);
    let requests = collector.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let trace = requests
        .iter()
        .find(|request| request.url.path() == TRACE_PATH)
        .unwrap();
    let log = requests
        .iter()
        .find(|request| request.url.path() == LOG_PATH)
        .unwrap();
    assert_private_signals(trace, log);

    available.store(false, Ordering::SeqCst);
    let background = home
        .configured(PRIVATE_INPUT, &endpoint, &untrusted_endpoint)
        .env("CX_TELEMETRY_DELIVERY", "background")
        .output()
        .await
        .unwrap();
    assert_eq!(background.status.code(), Some(2));
    assert_eq!(background.stdout, baseline.stdout);
    assert_eq!(background.stderr, baseline.stderr);
    let directory = fs::read_dir(home.0.join(".cx/telemetry"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let spool = Spool::new(directory.clone());
    let cooldown = directory.join("retry-after");
    tokio::time::timeout(Duration::from_secs(10), async {
        while !cooldown.is_file() || spool.try_lock().is_err() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("sender did not complete retry attempts");
    let batches = spool.pending().unwrap();
    assert_eq!(batches.len(), 1);
    let stored_bytes = fs::read(directory.join(format!("{}.otlp", batches[0].id))).unwrap();
    assert_eq!(stored_bytes, batches[0].payload);

    available.store(true, Ordering::SeqCst);
    fs::remove_file(cooldown).unwrap();
    let replay = home
        .configured("__telemetry-send", &endpoint, &untrusted_endpoint)
        .env("CX_TELEMETRY_DELIVERY", "background")
        .output()
        .await
        .unwrap();
    assert!(replay.status.success());
    assert!(replay.stdout.is_empty() && replay.stderr.is_empty());
    let requests = collector.received_requests().await.unwrap();
    assert_eq!(requests.len(), 7);
    assert_eq!(requests[5].url.path(), TRACE_PATH);
    assert_eq!(requests[6].url.path(), LOG_PATH);
    assert_eq!(requests[5].body, stored_bytes);
    assert_private_signals(&requests[5], &requests[6]);
    assert!(spool.pending().unwrap().is_empty());
    assert!(untrusted.received_requests().await.unwrap().is_empty());
}
