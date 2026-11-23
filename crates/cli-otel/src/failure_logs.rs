use std::collections::HashSet;

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value::Value, AnyValue, ArrayValue, KeyValue};
use opentelemetry_proto::tonic::logs::v1::{LogRecord, ResourceLogs, ScopeLogs, SeverityNumber};
use opentelemetry_proto::tonic::trace::v1::{span::SpanKind, status::StatusCode, Span};
use prost::Message;

use crate::limits::{flag, label, MAX_ARRAY_ITEMS, MAX_BATCH_BYTES, MAX_COMMAND_DEPTH};
use crate::{otlp, Url};

/// Resolve the standard OTLP logs sibling on the same origin. Paths and
/// parameters intended for a custom trace receiver are left alone.
pub fn command_failure_logs_endpoint(traces: &Url) -> Option<Url> {
    if !matches!(traces.scheme(), "http" | "https")
        || traces.host().is_none()
        || !traces.username().is_empty()
        || traces.password().is_some()
        || traces.path() != "/v1/traces"
        || traces.query().is_some()
        || traces.fragment().is_some()
    {
        return None;
    }
    let mut logs = traces.clone();
    logs.set_path("/v1/logs");
    Some(logs)
}

/// Derive failure records from a `cli-otel` trace batch. Applications replaying
/// queued bytes should apply their own privacy policy first, including to the
/// resource fields that are shared with traces.
///
/// Returns `None` for an invalid batch or one without a finalized failure.
pub fn encode_command_failure_logs(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.is_empty() || payload.len() > MAX_BATCH_BYTES {
        return None;
    }
    let request = ExportTraceServiceRequest::decode(payload).ok()?;
    if !otlp::valid_request(&request) {
        return None;
    }
    let span_ids: HashSet<_> = request
        .resource_spans
        .iter()
        .flat_map(|resource| &resource.scope_spans)
        .flat_map(|scope| &scope.spans)
        .map(|span| (span.trace_id.as_slice(), span.span_id.as_slice()))
        .collect();
    let mut logs = ExportLogsServiceRequest::default();
    for resource in &request.resource_spans {
        let mut scope_logs = Vec::new();
        for scope in &resource.scope_spans {
            if scope
                .scope
                .as_ref()
                .is_none_or(|scope| scope.name != "cli-otel")
            {
                continue;
            }
            let log_records: Vec<_> = scope
                .spans
                .iter()
                .filter(|span| {
                    // An incoming parent may not be in this batch. A local child
                    // of the command is never the invocation itself.
                    !span_ids.contains(&(span.trace_id.as_slice(), span.parent_span_id.as_slice()))
                })
                .filter_map(failure_record)
                .collect();
            if !log_records.is_empty() {
                scope_logs.push(ScopeLogs {
                    scope: scope.scope.clone(),
                    schema_url: scope.schema_url.clone(),
                    log_records,
                });
            }
        }
        if !scope_logs.is_empty() {
            logs.resource_logs.push(ResourceLogs {
                resource: resource.resource.clone(),
                schema_url: resource.schema_url.clone(),
                scope_logs,
            });
        }
    }
    (!logs.resource_logs.is_empty() && logs.encoded_len() <= MAX_BATCH_BYTES)
        .then(|| logs.encode_to_vec())
}

fn failure_record(span: &Span) -> Option<LogRecord> {
    if span.kind != SpanKind::Internal as i32
        || span
            .status
            .as_ref()
            .is_none_or(|status| status.code != StatusCode::Error as i32)
    {
        return None;
    }

    let (executable, Value::StringValue(name)) = field(span, "process.executable.name")? else {
        return None;
    };
    let (_, Value::IntValue(pid)) = field(span, "process.pid")? else {
        return None;
    };
    let (exit, Value::IntValue(code)) = field(span, "process.exit.code")? else {
        return None;
    };
    let (termination, Value::StringValue(reason)) = field(span, "cli.termination.reason")? else {
        return None;
    };
    let (error, Value::StringValue(category)) = field(span, "error.type")? else {
        return None;
    };
    if !label(name)
        || *pid <= 0
        || u32::try_from(*pid).is_err()
        || *code == 0
        || i32::try_from(*code).is_err()
        || !matches!(
            reason.as_str(),
            "completed" | "cancelled" | "timeout" | "signal"
        )
        || !label(category)
        || !matches!(field(span, "cli.mode"), Some((_, Value::StringValue(mode))) if mode == "oneshot")
    {
        return None;
    }

    let attributes = vec![
        executable.clone(),
        exit.clone(),
        termination.clone(),
        error.clone(),
        command_arguments(span, name)?,
    ];
    Some(LogRecord {
        time_unix_nano: span.end_time_unix_nano,
        severity_number: SeverityNumber::Error as i32,
        severity_text: "ERROR".into(),
        body: Some(AnyValue {
            value: Some(Value::StringValue("CLI command failed".into())),
        }),
        attributes,
        // OTLP spans can use the higher bits for parent context details.
        flags: span.flags & 0xff,
        trace_id: span.trace_id.clone(),
        span_id: span.span_id.clone(),
        ..LogRecord::default()
    })
}

fn command_arguments(span: &Span, executable: &str) -> Option<KeyValue> {
    let command: Vec<_> = span.name.split(' ').collect();
    if command.first().copied() != Some(executable)
        || command.len() > 1 + MAX_COMMAND_DEPTH
        || !command.iter().all(|token| label(token))
    {
        return None;
    }

    if let Some((attribute, value)) = field(span, "process.command_args") {
        let Value::ArrayValue(args) = value else {
            return None;
        };
        let flags = args.values.get(command.len()..)?;
        let prefix_matches = args.values.iter().zip(&command).all(
            |(arg, token)| matches!(&arg.value, Some(Value::StringValue(value)) if value == token),
        );
        return (prefix_matches && flags.iter().all(is_flag)).then(|| attribute.clone());
    }

    // Older queued traces used separate command and flag fields. Preserve
    // their trace bytes while deriving the new field for the failure log.
    if let Some((_, value)) = field(span, "cli.command") {
        if !matches!(value, Value::StringValue(path) if span.name.split_once(' ') == Some((executable, path)))
        {
            return None;
        }
    } else if command.len() != 1 {
        return None;
    }
    let mut values: Vec<_> = command
        .iter()
        .map(|token| AnyValue {
            value: Some(Value::StringValue((*token).to_owned())),
        })
        .collect();
    if let Some((_, value)) = field(span, "cli.flags") {
        let Value::ArrayValue(flags) = value else {
            return None;
        };
        if !flags.values.iter().all(is_flag) {
            return None;
        }
        values.extend(
            flags
                .values
                .iter()
                .take(MAX_ARRAY_ITEMS - command.len())
                .cloned(),
        );
    }
    Some(KeyValue {
        key: "process.command_args".into(),
        value: Some(AnyValue {
            value: Some(Value::ArrayValue(ArrayValue { values })),
        }),
        ..KeyValue::default()
    })
}

fn is_flag(arg: &AnyValue) -> bool {
    matches!(&arg.value, Some(Value::StringValue(value)) if flag(value))
}

fn field<'a>(span: &'a Span, key: &str) -> Option<(&'a KeyValue, &'a Value)> {
    let attribute = span
        .attributes
        .iter()
        .find(|attribute| attribute.key == key)?;
    Some((attribute, attribute.value.as_ref()?.value.as_ref()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_proto::tonic::common::v1::InstrumentationScope;
    use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Status};

    fn attribute(key: &str, value: Value) -> KeyValue {
        KeyValue {
            key: key.into(),
            value: Some(AnyValue { value: Some(value) }),
            ..KeyValue::default()
        }
    }

    fn failed_span() -> Span {
        Span {
            trace_id: vec![1; 16],
            span_id: vec![2; 8],
            parent_span_id: vec![3; 8],
            name: "tool check".into(),
            kind: SpanKind::Internal as i32,
            start_time_unix_nano: 10,
            end_time_unix_nano: 20,
            attributes: vec![
                attribute("process.executable.name", Value::StringValue("tool".into())),
                attribute("process.pid", Value::IntValue(10)),
                attribute("process.exit.code", Value::IntValue(1)),
                attribute("cli.mode", Value::StringValue("oneshot".into())),
                attribute("cli.command", Value::StringValue("check".into())),
                attribute(
                    "cli.termination.reason",
                    Value::StringValue("completed".into()),
                ),
                attribute("error.type", Value::StringValue("test_error".into())),
            ],
            status: Some(Status {
                code: StatusCode::Error as i32,
                message: String::new(),
            }),
            ..Span::default()
        }
    }

    fn batch(spans: Vec<Span>) -> Vec<u8> {
        ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                scope_spans: vec![ScopeSpans {
                    scope: Some(InstrumentationScope {
                        name: "cli-otel".into(),
                        ..InstrumentationScope::default()
                    }),
                    spans,
                    ..ScopeSpans::default()
                }],
                ..ResourceSpans::default()
            }],
        }
        .encode_to_vec()
    }

    #[test]
    fn selects_only_finalized_failed_command_spans() {
        let root = failed_span();
        let mut child = root.clone();
        child.span_id = vec![4; 8];
        child.parent_span_id = root.span_id.clone();
        let mut http = child.clone();
        http.span_id = vec![5; 8];
        http.kind = SpanKind::Client as i32;
        let logs =
            encode_command_failure_logs(&batch(vec![root.clone(), child.clone(), http.clone()]))
                .unwrap();
        let logs = ExportLogsServiceRequest::decode(logs.as_slice()).unwrap();
        let records = &logs.resource_logs[0].scope_logs[0].log_records;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].span_id, root.span_id);

        let mut successful = root.clone();
        successful.attributes[2] = attribute("process.exit.code", Value::IntValue(0));
        successful.status = None;
        assert!(encode_command_failure_logs(&batch(vec![successful, child, http])).is_none());

        let mut incomplete = root.clone();
        incomplete.attributes.remove(2);
        assert!(encode_command_failure_logs(&batch(vec![incomplete])).is_none());

        let mut malformed = root;
        malformed.attributes[2] = attribute("process.exit.code", Value::StringValue("1".into()));
        assert!(encode_command_failure_logs(&batch(vec![malformed])).is_none());
        assert!(encode_command_failure_logs(b"corrupt batch").is_none());
    }

    #[test]
    fn maps_only_standard_trace_urls_to_the_same_origin() {
        let traces = "https://collector.example.com:4318/v1/traces"
            .parse()
            .unwrap();
        assert_eq!(
            command_failure_logs_endpoint(&traces).unwrap().as_str(),
            "https://collector.example.com:4318/v1/logs"
        );
        for traces in [
            "https://collector.example.com/v1/traces/",
            "https://collector.example.com/v1/traces?private=true",
            "https://collector.example.com/v1/traces#fragment",
            "https://collector.example.com/collector/v1/traces",
            "https://user:password@collector.example.com/v1/traces",
        ] {
            assert!(command_failure_logs_endpoint(&traces.parse().unwrap()).is_none());
        }
    }
}
