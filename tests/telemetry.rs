use std::path::PathBuf;
use std::process::Output;
use std::time::{Duration, Instant};

use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value::Value, KeyValue};
use opentelemetry_proto::tonic::trace::v1::{
    span::SpanKind, status::StatusCode, ResourceSpans, Span,
};
use prost::Message;
use serde_json::json;
use tokio::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const TRACE_PATH: &str = "/v1/traces";
const TAGGED_PROFILE: &str = "telemetry-secret-profile";
const CUSTOMER_KEY: &str = "telemetry-secret-api-key";

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("cx-telemetry-{}", uuid::Uuid::new_v4())))
    }

    fn cx(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cx"));
        command
            .env_clear()
            .env("CX_HOME", &self.0)
            .env("CX_TELEMETRY_ENDPOINT", "")
            .env("CX_NO_UPDATE_NOTIFIER", "1")
            .env("CX_NO_SKILLS_AUTO_UPDATE", "1")
            .args(args);
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        command
    }

    fn profile(&self, name: &str, endpoint: &str) {
        let directory = self.0.join(".cx/profiles");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{name}.toml")),
            format!(
                "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"{CUSTOMER_KEY}\"\nregion = \"{endpoint}\"\n"
            ),
        )
        .unwrap();
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn endpoint(server: &MockServer) -> String {
    format!("{}{TRACE_PATH}", server.uri())
}

async fn receiver(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

async fn received(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap()
}

fn decode(request: &Request) -> ResourceSpans {
    assert_eq!(request.url.path(), TRACE_PATH);
    assert_eq!(request.headers["content-type"], "application/x-protobuf");
    let mut data = ExportTraceServiceRequest::decode(request.body.as_slice()).unwrap();
    assert_eq!(data.resource_spans.len(), 1);
    data.resource_spans.pop().unwrap()
}

fn attr<'a>(attrs: &'a [KeyValue], name: &str) -> Option<&'a Value> {
    attrs
        .iter()
        .find(|attr| attr.key == name)?
        .value
        .as_ref()?
        .value
        .as_ref()
}

fn text<'a>(attrs: &'a [KeyValue], name: &str) -> Option<&'a str> {
    match attr(attrs, name) {
        Some(Value::StringValue(text)) => Some(text),
        _ => None,
    }
}

fn int(attrs: &[KeyValue], name: &str) -> Option<i64> {
    match attr(attrs, name) {
        Some(Value::IntValue(value)) => Some(*value),
        _ => None,
    }
}

fn assert_command_args(attrs: &[KeyValue], expected: &[&str]) {
    assert!(attr(attrs, "cli.command").is_none());
    assert!(attr(attrs, "cli.flags").is_none());
    let Some(Value::ArrayValue(args)) = attr(attrs, "process.command_args") else {
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
    assert_eq!(args, expected);
}

fn command_span(data: &ResourceSpans) -> &Span {
    let mut candidates = data
        .scope_spans
        .iter()
        .flat_map(|scope| &scope.spans)
        .filter(|span| span.kind == SpanKind::Internal as i32);
    let command = candidates.next().expect("command span");
    assert!(candidates.next().is_none());
    command
}

fn assert_output_matches(enabled: &Output, baseline: &Output) {
    assert_eq!(enabled.status, baseline.status);
    assert_eq!(enabled.stdout, baseline.stdout);
    assert_eq!(enabled.stderr, baseline.stderr);
}

fn assert_private(request: &Request, sentinels: &[&str]) {
    for sentinel in sentinels {
        assert!(
            !request
                .body
                .windows(sentinel.len())
                .any(|bytes| bytes == sentinel.as_bytes()),
            "telemetry exported a sentinel"
        );
    }
}

#[tokio::test]
async fn exports_safe_command_metadata_with_explicit_configuration_and_parent() {
    let home = Sandbox::new();
    let collector = receiver(ResponseTemplate::new(200)).await;
    let wrong_collector = receiver(ResponseTemplate::new(200)).await;
    let secret_label = "telemetry-secret-label";
    let baseline = home
        .cx(&["profile", "list", "--label", secret_label])
        .output()
        .await
        .unwrap();
    let actual = home
        .cx(&["profile", "list", "--label", secret_label])
        .env("CX_TELEMETRY_ENDPOINT", endpoint(&collector))
        .env("CX_TELEMETRY_API_KEY", "private-collector-token")
        .env("CLAUDE_CODE", "agent-env-value-must-not-leak")
        .env("GITHUB_ACTIONS", "true")
        .env(
            "TRACEPARENT",
            "00-11111111111111111111111111111111-2222222222222222-01",
        )
        .env("TRACESTATE", "example=vendor")
        .env("BAGGAGE", "hidden=telemetry-secret-baggage")
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", wrong_collector.uri())
        .env(
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
            endpoint(&wrong_collector),
        )
        .env("OTEL_EXPORTER_OTLP_HEADERS", "x-hostile=secret-header")
        .env(
            "OTEL_EXPORTER_OTLP_TRACES_HEADERS",
            "authorization=hostile-token",
        )
        .env("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc")
        .env("OTEL_SDK_DISABLED", "true")
        .env("OTEL_SERVICE_NAME", "hostile-name")
        .env("OTEL_RESOURCE_ATTRIBUTES", "hidden=hostile-resource")
        .env("OTEL_TRACES_SAMPLER", "always_off")
        .env("OTEL_SPAN_ATTRIBUTE_COUNT_LIMIT", "1")
        .output()
        .await
        .unwrap();
    assert!(actual.status.success());
    assert_output_matches(&actual, &baseline);
    assert!(received(&wrong_collector).await.is_empty());
    let requests = received(&collector).await;
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.headers["authorization"],
        "Bearer private-collector-token"
    );
    assert!(!request.headers.contains_key("x-hostile"));
    let data = decode(request);
    let resource = &data.resource.as_ref().unwrap().attributes;
    assert_eq!(text(resource, "service.name"), Some("cx"));
    assert_eq!(
        text(resource, "service.version"),
        Some(env!("CARGO_PKG_VERSION"))
    );
    for key in ["os.type", "host.arch", "cli.installation.method"] {
        assert!(text(resource, key).is_some(), "missing {key}");
    }
    assert!(uuid::Uuid::parse_str(text(resource, "app.installation.id").unwrap()).is_ok());
    let command = command_span(&data);
    assert_eq!(command.name, "cx profiles list");
    assert_eq!(command.trace_id, [0x11; 16]);
    assert_eq!(command.parent_span_id, [0x22; 8]);
    assert_eq!(command.trace_state, "example=vendor");
    assert_eq!(
        command.status.as_ref().unwrap().code,
        StatusCode::Unset as i32
    );
    assert_eq!(int(&command.attributes, "process.exit.code"), Some(0));
    assert!(int(&command.attributes, "process.pid").unwrap() > 0);
    assert_eq!(
        attr(&command.attributes, "process.interactive"),
        Some(&Value::BoolValue(false))
    );
    assert_eq!(
        text(&command.attributes, "process.executable.name"),
        Some("cx")
    );
    assert_eq!(text(&command.attributes, "cli.mode"), Some("oneshot"));
    assert_eq!(
        text(&command.attributes, "cli.ci.provider"),
        Some("github_actions")
    );
    assert_eq!(text(&command.attributes, "cli.output.format"), Some("text"));
    assert_eq!(
        text(&command.attributes, "gen_ai.agent.name"),
        Some("claude-code")
    );
    assert_eq!(
        text(&command.attributes, "cli.termination.reason"),
        Some("completed")
    );
    assert_command_args(&command.attributes, &["cx", "profiles", "list", "--label"]);
    assert!(attr(&command.attributes, "error.type").is_none());
    assert_private(
        request,
        &[
            secret_label,
            "telemetry-secret-baggage",
            "private-collector-token",
            "agent-env-value-must-not-leak",
            "hostile-",
        ],
    );
}

#[tokio::test]
async fn exports_single_spans_for_help_parse_errors_and_first_run_failure() {
    let home = Sandbox::new();
    let collector = receiver(ResponseTemplate::new(200)).await;
    let secret = "telemetry-secret-invalid-input";
    let cases: &[(&[&str], i32, &str, Option<&str>)] = &[
        (&["--help"], 0, "cx", None),
        (&["profile", "--help"], 0, "cx", None),
        (&["--version"], 0, "cx", None),
        (&[secret], 2, "cx", Some("argument_parse")),
        (&["logs", secret], 1, "cx logs", Some("configuration")),
    ];
    for (args, code, _, _) in cases {
        let baseline = home.cx(args).output().await.unwrap();
        let actual = home
            .cx(args)
            .env("CX_TELEMETRY_ENDPOINT", endpoint(&collector))
            .output()
            .await
            .unwrap();
        assert_eq!(actual.status.code(), Some(*code));
        assert_output_matches(&actual, &baseline);
    }

    let requests = received(&collector).await;
    assert_eq!(requests.len(), cases.len());
    for (request, (_, code, name, category)) in requests.iter().zip(cases) {
        assert_private(request, &[secret]);
        let data = decode(request);
        let command = command_span(&data);
        assert_eq!(command.name, *name);
        assert_eq!(
            int(&command.attributes, "process.exit.code"),
            Some(i64::from(*code))
        );
        assert_eq!(text(&command.attributes, "error.type"), *category);
        let status = command.status.as_ref().unwrap();
        assert_eq!(
            status.code,
            if *code == 0 {
                StatusCode::Unset
            } else {
                StatusCode::Error
            } as i32
        );
        assert!(status.message.is_empty());
    }
}

#[tokio::test]
async fn export_failure_and_stall_preserve_output_and_opt_out_skips_initialization() {
    let home = Sandbox::new();
    let failed = receiver(ResponseTemplate::new(503)).await;
    let stalled = receiver(ResponseTemplate::new(200).set_delay(Duration::from_secs(10))).await;
    let args = &["profiles", "list"];
    let baseline = home.cx(args).output().await.unwrap();

    for (variable, value) in [("CX_TELEMETRY", "off"), ("DO_NOT_TRACK", "1")] {
        let output = home
            .cx(args)
            .env("CX_TELEMETRY_ENDPOINT", endpoint(&failed))
            .env(variable, value)
            .output()
            .await
            .unwrap();
        assert_output_matches(&output, &baseline);
    }
    assert!(received(&failed).await.is_empty());
    assert!(!home.0.join(".cx/state.json").exists());

    let started = Instant::now();
    let output = home
        .cx(args)
        .env("CX_TELEMETRY_ENDPOINT", endpoint(&stalled))
        .output()
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_output_matches(&output, &baseline);
    assert_eq!(received(&stalled).await.len(), 1);

    let output = home
        .cx(args)
        .env("CX_TELEMETRY_ENDPOINT", endpoint(&failed))
        .output()
        .await
        .unwrap();
    assert_output_matches(&output, &baseline);
    assert_eq!(received(&failed).await.len(), 1);
}

#[tokio::test]
async fn http_fan_out_uses_the_command_parent_and_keeps_only_safe_metadata() {
    let home = Sandbox::new();
    let collector = receiver(ResponseTemplate::new(200)).await;
    let api = MockServer::start().await;
    home.profile(TAGGED_PROFILE, &api.uri());
    home.profile("telemetry-second-profile", &api.uri());

    Mock::given(method("GET"))
        .and(path("/metrics/api/v1/query"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(50))
                .set_body_json(json!({
                    "status": "success",
                    "data": { "resultType": "vector", "result": [] }
                })),
        )
        .expect(2)
        .mount(&api)
        .await;
    let query = "telemetry-secret-query";
    let result = home
        .cx(&[
            "metrics",
            "query",
            query,
            "-p",
            TAGGED_PROFILE,
            "-p",
            "telemetry-second-profile",
            "--no-console-link",
            "-o",
            "json",
        ])
        .env("CX_HTTP_TIMEOUT", "4")
        .env("CX_TELEMETRY_ENDPOINT", endpoint(&collector))
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let requests = received(&collector).await;
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_private(
        request,
        &[
            TAGGED_PROFILE,
            "telemetry-second-profile",
            CUSTOMER_KEY,
            query,
        ],
    );
    let data = decode(request);
    let command = command_span(&data);
    assert_eq!(command.name, "cx metrics query");
    assert_eq!(
        text(&command.attributes, "cx.cli.query.language"),
        Some("promql")
    );
    assert_eq!(int(&command.attributes, "cx.cli.profile.count"), Some(2));
    assert_eq!(text(&command.attributes, "cli.output.format"), Some("json"));
    assert_command_args(
        &command.attributes,
        &[
            "cx",
            "metrics",
            "query",
            "--no-console-link",
            "--output",
            "--profile",
        ],
    );

    let children: Vec<_> = data
        .scope_spans
        .iter()
        .flat_map(|scope| &scope.spans)
        .filter(|span| span.kind == SpanKind::Client as i32)
        .collect();
    assert_eq!(children.len(), 2);
    for child in &children {
        assert_eq!(child.name, "GET");
        assert_eq!(child.parent_span_id, command.span_id);
        assert_eq!(child.trace_id, command.trace_id);
        assert_eq!(text(&child.attributes, "http.request.method"), Some("GET"));
        assert_eq!(text(&child.attributes, "server.address"), Some("127.0.0.1"));
        assert_eq!(
            int(&child.attributes, "http.response.status_code"),
            Some(200)
        );
        assert_eq!(
            text(&child.attributes, "url.template"),
            Some("/metrics/api/v1/query")
        );
    }
    assert_ne!(children[0].span_id, children[1].span_id);
    assert!(children[0].start_time_unix_nano < children[1].end_time_unix_nano);
    assert!(children[1].start_time_unix_nano < children[0].end_time_unix_nano);
}

#[tokio::test]
async fn http_failure_records_status_without_dynamic_route_or_error_message() {
    let home = Sandbox::new();
    let collector = receiver(ResponseTemplate::new(200)).await;
    let api = MockServer::start().await;
    let id = "telemetry-secret-dynamic-id";
    let server_error = "telemetry-secret-error-body";
    let endpoint_path = format!("/mgmt/openapi/5/data-exploration/views/v1/views/{id}");
    Mock::given(method("GET"))
        .and(path(endpoint_path))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({ "message": server_error })))
        .expect(1)
        .mount(&api)
        .await;

    let output = home
        .cx(&["views", "get", id, "--no-console-link"])
        .env("CX_REGION", api.uri())
        .env("CX_API_KEY", CUSTOMER_KEY)
        .env("CX_TELEMETRY_ENDPOINT", endpoint(&collector))
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(server_error));
    let requests = received(&collector).await;
    assert_eq!(requests.len(), 1);
    assert_private(&requests[0], &[id, CUSTOMER_KEY, server_error]);
    let data = decode(&requests[0]);
    let command = command_span(&data);
    assert_eq!(int(&command.attributes, "process.exit.code"), Some(1));
    assert_eq!(int(&command.attributes, "cx.cli.profile.count"), Some(1));
    assert!(attr(&command.attributes, "cli.ci.provider").is_none());
    let child = data
        .scope_spans
        .iter()
        .flat_map(|scope| &scope.spans)
        .find(|span| span.kind == SpanKind::Client as i32)
        .unwrap();
    assert_eq!(child.parent_span_id, command.span_id);
    assert_eq!(
        int(&child.attributes, "http.response.status_code"),
        Some(403)
    );
    assert_eq!(text(&child.attributes, "error.type"), Some("403"));
    assert_eq!(
        child.status.as_ref().unwrap().code,
        StatusCode::Error as i32
    );
    assert!(attr(&child.attributes, "url.template").is_none());
}
