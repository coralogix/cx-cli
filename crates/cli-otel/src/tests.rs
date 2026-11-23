use super::*;
use opentelemetry::trace::{SpanId, TraceId};

#[cfg(feature = "clap")]
#[test]
fn clap_metadata_uses_canonical_names_and_only_explicit_flags() {
    use clap::{Arg, ArgAction, Command};

    let mut parser = Command::new("tool")
        .arg(Arg::new("profile").short('p').long("profile").global(true))
        .subcommand(
            Command::new("profiles").alias("profile").subcommand(
                Command::new("list")
                    .alias("ls")
                    .arg(Arg::new("quiet").short('q').action(ArgAction::SetTrue))
                    .arg(Arg::new("output").long("output").default_value("text"))
                    .arg(Arg::new("position")),
            ),
        );
    let matches = parser
        .try_get_matches_from_mut(["tool", "profile", "ls", "-qp", "secret", "literal"])
        .unwrap();
    let metadata = CommandMetadata::from_clap(&parser, &matches);
    assert_eq!(metadata.command_path, ["profiles", "list"]);
    assert_eq!(metadata.flags, ["--profile", "-q"]);
}

#[tokio::test]
async fn spans_follow_trace_context_and_buffer_keeps_the_command() {
    let telemetry = Telemetry::new(Config::new(
        "http://127.0.0.1:4318/v1/traces".parse().unwrap(),
        "tool",
        "1.2.3",
    ))
    .unwrap();
    let parent = ParentTrace {
        traceparent: Some("00-11111111111111111111111111111111-2222222222222222-01"),
        tracestate: Some("example=vendor"),
    };
    let result = telemetry
        .run(
            CommandMetadata {
                command_path: vec!["request".into()],
                flags: vec!["--quiet".into()],
            },
            parent,
            async {
                tokio::task::yield_now().await;
                record_attribute(KeyValue::new("app.allowed", true));
                for _ in 0..MAX_SPANS + 20 {
                    let mut child = start_child_span("GET", SpanKind::Client, Vec::new).unwrap();
                    child.end();
                }
                19
            },
            |result| Outcome {
                error_type: Some("cancelled"),
                termination: Termination::Cancelled,
                ..Outcome::completed(*result)
            },
        )
        .await;

    assert_eq!(result, 19);
    let spans = telemetry.buffer.drain();
    assert_eq!(spans.len(), MAX_SPANS);
    let root = spans.last().unwrap();
    assert_eq!(root.name, "tool request");
    assert_eq!(root.span_kind, SpanKind::Internal);
    assert_eq!(
        root.span_context.trace_id(),
        TraceId::from_hex("11111111111111111111111111111111").unwrap()
    );
    assert_eq!(
        root.parent_span_id,
        SpanId::from_hex("2222222222222222").unwrap()
    );
    assert_eq!(
        root.span_context.trace_state().get("example"),
        Some("vendor")
    );
    assert_eq!(root.status, Status::error(""));
    assert!(root
        .attributes
        .contains(&KeyValue::new("process.exit.code", 19)));
    assert!(root
        .attributes
        .contains(&KeyValue::new("app.allowed", true)));
    assert!(root
        .attributes
        .contains(&KeyValue::new("cli.termination.reason", "cancelled")));
    assert!(root
        .attributes
        .contains(&KeyValue::new("error.type", "cancelled")));
    for span in &spans[..spans.len() - 1] {
        assert_eq!(span.span_kind, SpanKind::Client);
        assert_eq!(span.parent_span_id, root.span_context.span_id());
        assert_eq!(span.span_context.trace_id(), root.span_context.trace_id());
    }
}

#[test]
fn extension_callbacks_are_lazy_outside_an_invocation() {
    record_command_metadata(|| panic!("metadata evaluated outside an invocation"));
    assert!(start_child_span("GET", SpanKind::Client, || {
        panic!("attributes evaluated outside an invocation")
    })
    .is_none());
}
