use std::process::Command;

use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use prost::Message;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn base_command(home: &TempDir) -> Command {
    let mut cx = Command::new(env!("CARGO_BIN_EXE_cx"));
    cx.env_clear()
        .env("CX_HOME", home.path())
        .env("CX_NO_UPDATE_NOTIFIER", "1")
        .env("CX_NO_SKILLS_AUTO_UPDATE", "1");
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        cx.env("SystemRoot", system_root);
    }
    cx
}

#[tokio::test]
async fn telemetry_v1_posts_single_span_to_loopback() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let home = TempDir::new().expect("temp home");
    let mut cx = base_command(&home);
    cx.env("CX_TELEMETRY", "1")
        .env("CX_OTLP_ENDPOINT", format!("{}/v1/traces", server.uri()))
        .env("CX_OTLP_API_KEY", "local-test-key")
        .args(["--profile", "private-profile-sentinel", "schema"]);

    let output = cx.output().expect("run cx");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = server.received_requests().await.expect("captured requests");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(!request.headers.contains_key("authorization"));
    let trace = ExportTraceServiceRequest::decode(request.body.as_slice()).expect("OTLP trace");
    assert_eq!(trace.resource_spans.len(), 1);
    assert!(!format!("{trace:?}").contains("private-profile-sentinel"));
    assert!(!format!("{trace:?}").contains("local-test-key"));
}

#[tokio::test]
async fn telemetry_v1_skips_export_when_do_not_track() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let home = TempDir::new().expect("temp home");
    let mut cx = base_command(&home);
    cx.env("CX_TELEMETRY", "1")
        .env("DO_NOT_TRACK", "1")
        .env("CX_OTLP_ENDPOINT", format!("{}/v1/traces", server.uri()))
        .env("CX_OTLP_API_KEY", "local-test-key")
        .arg("schema");

    let output = cx.output().expect("run cx");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn telemetry_v1_skips_export_when_cx_telemetry_unset() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let home = TempDir::new().expect("temp home");
    let mut cx = base_command(&home);
    // Endpoint alone must not enable export; CX_TELEMETRY stays unset.
    cx.env("CX_OTLP_ENDPOINT", format!("{}/v1/traces", server.uri()))
        .env("CX_OTLP_API_KEY", "local-test-key")
        .arg("schema");

    let output = cx.output().expect("run cx");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn telemetry_classifies_help_version_and_invalid_arguments() {
    for (argument, expected_exit, is_error) in [
        ("--help", 0, false),
        ("--version", 0, false),
        ("--not-a-valid-option", 2, true),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/traces"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let home = TempDir::new().expect("temp home");
        let output = base_command(&home)
            .env("CX_TELEMETRY", "1")
            .env("CX_OTLP_ENDPOINT", format!("{}/v1/traces", server.uri()))
            .arg(argument)
            .output()
            .expect("run cx");
        assert_eq!(
            output.status.code(),
            Some(expected_exit),
            "argument: {argument}"
        );
        let requests = server.received_requests().await.expect("captured requests");
        assert_eq!(requests.len(), 1);
        let trace =
            ExportTraceServiceRequest::decode(requests[0].body.as_slice()).expect("OTLP trace");
        let spans = &trace.resource_spans[0].scope_spans[0].spans;
        assert_eq!(spans.len(), 1);
        let attrs = &spans[0].attributes;
        assert!(attrs.iter().any(|kv| kv.key == "process.exit.code" && matches!(kv.value.as_ref().and_then(|v| v.value.as_ref()), Some(Value::IntValue(code)) if *code == i64::from(expected_exit))));
        let error = attrs.iter().find(|kv| kv.key == "error.type");
        if is_error {
            assert!(
                matches!(error.and_then(|kv| kv.value.as_ref()).and_then(|v| v.value.as_ref()), Some(Value::StringValue(value)) if value == "argument_parse")
            );
        } else {
            assert!(error.is_none(), "{argument} should not be an error");
        }
    }
}
