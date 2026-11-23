use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cli_otel::Spool;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value::Value, AnyValue, ArrayValue, KeyValue};
use opentelemetry_proto::tonic::trace::v1::Span;
use prost::Message;
use tokio::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const ARGS: &str = "process.command_args";
const TRACE_PATH: &str = "/v1/traces";
const LOG_PATH: &str = "/v1/logs";
const PRIVATE_PROFILE: &str = "telemetry-private-profile-name";

struct Home(PathBuf);

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Home {
    fn cx(&self, args: &[&str], endpoint: &str) -> Command {
        let mut cx = Command::new(env!("CARGO_BIN_EXE_cx"));
        cx.env_clear()
            .env("CX_HOME", &self.0)
            .env("CX_NO_UPDATE_NOTIFIER", "1")
            .env("CX_NO_SKILLS_AUTO_UPDATE", "1")
            .env("CX_TELEMETRY_ENDPOINT", endpoint)
            .env("CX_TELEMETRY_FAILURE_LOGS", "1")
            .kill_on_drop(true)
            .args(args);
        cx
    }
}

fn field(key: &str, value: Value) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue { value: Some(value) }),
        ..KeyValue::default()
    }
}

fn strings(values: &[&str]) -> Value {
    Value::ArrayValue(ArrayValue {
        values: values
            .iter()
            .map(|value| AnyValue {
                value: Some(Value::StringValue((*value).to_owned())),
            })
            .collect(),
    })
}

fn assert_args(attributes: &[KeyValue], expected: &Value) {
    assert!(attributes
        .iter()
        .all(|attr| !matches!(attr.key.as_str(), "cli.command" | "cli.flags")));
    let arg = attributes.iter().find(|attr| attr.key == ARGS).unwrap();
    assert_eq!(arg.value.as_ref().unwrap().value.as_ref(), Some(expected));
}

fn assert_log_matches(logs: &[u8], span: &Span, expected: &Value) {
    let logs = ExportLogsServiceRequest::decode(logs).unwrap();
    let records = &logs.resource_logs[0].scope_logs[0].log_records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].trace_id, span.trace_id);
    assert_eq!(records[0].span_id, span.span_id);
    assert_args(&records[0].attributes, expected);
}

#[tokio::test]
async fn command_arguments_are_sanitized_and_legacy_queue_replays_without_rewriting() {
    let home = Home(std::env::temp_dir().join(format!("cx-command-args-{}", uuid::Uuid::new_v4())));
    let collector = MockServer::start().await;
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

    // A flag-looking positional value must stay out of the telemetry even
    // though the spelling matches a declared option.
    let output = home
        .cx(
            &[
                "-p",
                PRIVATE_PROFILE,
                "-o",
                "json",
                "logs",
                "--",
                "--no-console-link",
            ],
            &endpoint,
        )
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
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
    let request = ExportTraceServiceRequest::decode(trace.body.as_slice()).unwrap();
    let span = &request.resource_spans[0].scope_spans[0].spans[0];
    assert_eq!(span.name, "cx logs");
    let expected = strings(&["cx", "logs", "--output", "--profile"]);
    assert_args(&span.attributes, &expected);
    assert_log_matches(&log.body, span, &expected);
    for request in [&trace.body, &log.body] {
        for value in [PRIVATE_PROFILE, "--no-console-link"] {
            assert!(!request
                .windows(value.len())
                .any(|slice| slice == value.as_bytes()));
        }
    }

    // Create a real destination partition and hold the sender in cooldown
    // while preparing one legacy batch and one edited current batch.
    available.store(false, Ordering::SeqCst);
    let background = |args: &[&str]| {
        let mut cx = home.cx(args, &endpoint);
        cx.env("CX_TELEMETRY_DELIVERY", "background");
        cx
    };
    assert!(background(&["--version"])
        .output()
        .await
        .unwrap()
        .status
        .success());
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
    .expect("sender did not enter cooldown");
    for batch in spool.pending().unwrap() {
        spool.remove(&batch.id).unwrap();
    }

    let mut edited = request.clone();
    let edited_attrs = &mut edited.resource_spans[0].scope_spans[0].spans[0].attributes;
    edited_attrs.retain(|attr| attr.key != ARGS);
    edited_attrs.push(field(
        ARGS,
        strings(&["cx", "logs", "--undeclared-private-token"]),
    ));
    spool.enqueue(&edited.encode_to_vec()).unwrap();

    let mut legacy = request;
    let attrs = &mut legacy.resource_spans[0].scope_spans[0].spans[0].attributes;
    attrs.retain(|attr| attr.key != ARGS);
    attrs.push(field("cli.command", Value::StringValue("logs".into())));
    attrs.push(field("cli.flags", strings(&["--output", "--profile"])));
    let legacy_bytes = legacy.encode_to_vec();
    spool.enqueue(&legacy_bytes).unwrap();

    let mut retired = legacy.clone();
    let retired_span = &mut retired.resource_spans[0].scope_spans[0].spans[0];
    retired_span.name = "cx retired-command".to_owned();
    retired_span
        .attributes
        .iter_mut()
        .find(|attr| attr.key == "cli.command")
        .unwrap()
        .value = Some(AnyValue {
        value: Some(Value::StringValue("retired-command".into())),
    });
    let retired_bytes = retired.encode_to_vec();
    spool.enqueue(&retired_bytes).unwrap();

    available.store(true, Ordering::SeqCst);
    fs::remove_file(cooldown).unwrap();
    assert!(background(&["__telemetry-send"])
        .output()
        .await
        .unwrap()
        .status
        .success());
    let requests = collector.received_requests().await.unwrap();
    let replayed = &requests[5..];
    assert_eq!(replayed.len(), 3);
    assert!(replayed
        .iter()
        .any(|request| request.url.path() == TRACE_PATH && request.body == legacy_bytes));
    assert!(replayed
        .iter()
        .any(|request| request.url.path() == TRACE_PATH && request.body == retired_bytes));
    let log = replayed
        .iter()
        .find(|request| request.url.path() == LOG_PATH)
        .unwrap();
    assert_log_matches(
        &log.body,
        &legacy.resource_spans[0].scope_spans[0].spans[0],
        &expected,
    );
    assert!(spool.pending().unwrap().is_empty());
}
