use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value::Value, AnyValue, KeyValue};
use opentelemetry_proto::tonic::trace::v1::{span::SpanKind, status::StatusCode, Span};
use prost::Message;

use super::{AgentCaller, CiProvider, AGENT_ENV_VARS, HTTP_TEMPLATES};

/// The generic spool validates OTLP structure. Check cx's field policy as well
/// before a batch read from disk can go back on the wire.
pub(super) fn validate(payload: &[u8], schema: &clap::Command) -> Option<ReplayPolicy> {
    let request = ExportTraceServiceRequest::decode(payload).ok()?;
    let mut policy = ReplayPolicy {
        allow_failure_logs: true,
    };
    let valid = request.resource_spans.iter().all(|resource| {
        resource.schema_url.is_empty()
            && resource.resource.as_ref().is_some_and(|resource| {
                let attributes = &resource.attributes;
                attribute_value(attributes, "service.name").is_some()
                    && attribute_value(attributes, "service.version").is_some()
                    && attributes.iter().all(resource_attribute)
            })
            && resource.scope_spans.iter().all(|scope| {
                scope.schema_url.is_empty()
                    && scope.scope.as_ref().is_some_and(|scope| {
                        scope.name == "cli-otel"
                            && version(&scope.version)
                            && scope.attributes.is_empty()
                    })
                    && scope
                        .spans
                        .iter()
                        .all(|span| safe_span(span, schema, &mut policy))
            })
    });
    valid.then_some(policy)
}

pub(super) struct ReplayPolicy {
    pub(super) allow_failure_logs: bool,
}

fn version(value: &str) -> bool {
    value.len() <= 64
        && value.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+-".contains(&byte))
}

fn label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn command_path(value: &str) -> bool {
    let mut commands = value.split(' ');
    commands.clone().count() <= 8 && commands.all(label)
}

fn flag(value: &str) -> bool {
    if let Some(long) = value.strip_prefix("--") {
        return label(long);
    }
    value
        .strip_prefix('-')
        .is_some_and(|short| short.len() == 1 && label(short))
}

fn field(attribute: &KeyValue) -> (&str, Option<&Value>) {
    (
        attribute.key.as_str(),
        attribute
            .value
            .as_ref()
            .and_then(|value| value.value.as_ref()),
    )
}

fn attribute_value<'a>(attributes: &'a [KeyValue], name: &str) -> Option<&'a Value> {
    field(attributes.iter().find(|attribute| attribute.key == name)?).1
}

fn resource_attribute(attribute: &KeyValue) -> bool {
    let (key, Some(Value::StringValue(value))) = field(attribute) else {
        return false;
    };
    match key {
        "service.name" => value == "cx",
        "service.version" => version(value),
        "os.type" => value == "darwin" || value == std::env::consts::OS,
        "host.arch" => value == std::env::consts::ARCH || super::architecture_name(value).is_some(),
        "os.version" => {
            value.len() <= 64
                && value
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        }
        "app.installation.id" => uuid::Uuid::parse_str(value).is_ok(),
        "cli.installation.method" => {
            matches!(value.as_str(), "brew" | "install_script" | "unknown")
        }
        "cx.application.name" => value == "cx-cli",
        "cx.subsystem.name" => value == "usage",
        _ => false,
    }
}

fn safe_span(span: &Span, schema: &clap::Command, policy: &mut ReplayPolicy) -> bool {
    if !span.events.is_empty() || !span.links.is_empty() {
        return false;
    }
    match SpanKind::try_from(span.kind) {
        Ok(SpanKind::Internal) => {
            (span.name == "cx" || span.name.strip_prefix("cx ").is_some_and(command_path))
                && span.attributes.iter().all(command_attribute)
                && valid_command_outcome(span)
                && safe_command_args(span, schema, policy)
        }
        Ok(SpanKind::Client) => {
            (span.name == "HTTP" || http_method(&span.name))
                && span.attributes.iter().all(http_attribute)
        }
        _ => false,
    }
}

fn safe_command_args(span: &Span, schema: &clap::Command, policy: &mut ReplayPolicy) -> bool {
    let parts: Vec<_> = span.name.split(' ').collect();
    let mut commands = vec![schema];
    for name in &parts[1..] {
        let Some(command) = commands
            .last()
            .and_then(|command| command.find_subcommand(name))
        else {
            // An older trace may describe a command that is no longer declared.
            // Its bytes are still replayable under the legacy policy.
            policy.allow_failure_logs = false;
            return attribute_value(&span.attributes, "process.command_args").is_none();
        };
        if command.get_name() != *name {
            policy.allow_failure_logs = false;
            return attribute_value(&span.attributes, "process.command_args").is_none();
        }
        commands.push(command);
    }

    if let Some(Value::ArrayValue(args)) = attribute_value(&span.attributes, "process.command_args")
    {
        let Some(flags) = args.values.get(parts.len()..) else {
            return false;
        };
        return args.values.iter().zip(parts).all(
            |(arg, part)| matches!(&arg.value, Some(Value::StringValue(value)) if value == part),
        ) && declared_flags(flags, &commands)
            && attribute_value(&span.attributes, "cli.command").is_none()
            && attribute_value(&span.attributes, "cli.flags").is_none();
    }

    if let Some(Value::ArrayValue(flags)) = attribute_value(&span.attributes, "cli.flags") {
        policy.allow_failure_logs &= declared_flags(&flags.values, &commands);
    }
    true
}

fn declared_flags(flags: &[AnyValue], commands: &[&clap::Command]) -> bool {
    flags.iter().all(|value| {
        let Some(Value::StringValue(value)) = &value.value else {
            return false;
        };
        flag(value)
            && commands
                .iter()
                .flat_map(|command| command.get_arguments())
                .any(|arg| {
                    if let Some(name) = value.strip_prefix("--") {
                        arg.get_long() == Some(name)
                    } else {
                        arg.get_long().is_none()
                            && value.strip_prefix('-').and_then(|name| name.parse().ok())
                                == arg.get_short()
                    }
                })
    })
}

fn valid_command_outcome(span: &Span) -> bool {
    let attributes = &span.attributes;
    if attribute_value(attributes, "process.executable.name").is_none()
        || attribute_value(attributes, "process.pid").is_none()
    {
        return false;
    }
    let Some(Value::IntValue(exit_code)) = attribute_value(attributes, "process.exit.code") else {
        return false;
    };
    let status = span
        .status
        .as_ref()
        .map_or(StatusCode::Unset as i32, |status| status.code);
    let has_error_type = attribute_value(attributes, "error.type").is_some();
    if *exit_code == 0 {
        status == StatusCode::Unset as i32 && !has_error_type
    } else {
        status == StatusCode::Error as i32 && has_error_type
    }
}

fn command_attribute(attribute: &KeyValue) -> bool {
    let (key, value) = field(attribute);
    match (key, value) {
        ("process.executable.name", Some(Value::StringValue(value))) => value == "cx",
        ("process.pid", Some(Value::IntValue(value))) => {
            *value > 0 && u32::try_from(*value).is_ok()
        }
        ("process.exit.code", Some(Value::IntValue(value))) => i32::try_from(*value).is_ok(),
        ("process.interactive" | "cx.cli.command.risky", Some(Value::BoolValue(_))) => true,
        ("cx.cli.profile.count", Some(Value::IntValue(value))) => *value >= 0,
        ("cli.mode", Some(Value::StringValue(value))) => value == "oneshot",
        ("process.command_args", Some(Value::ArrayValue(_))) => true,
        ("cli.command", Some(Value::StringValue(value))) => command_path(value),
        ("cli.flags", Some(Value::ArrayValue(values))) => values
            .values
            .iter()
            .all(|value| matches!(&value.value, Some(Value::StringValue(value)) if flag(value))),
        ("cli.termination.reason", Some(Value::StringValue(value))) => {
            matches!(
                value.as_str(),
                "completed" | "cancelled" | "timeout" | "signal"
            )
        }
        ("cli.output.format", Some(Value::StringValue(value))) => {
            matches!(value.as_str(), "text" | "json" | "toon")
        }
        ("cli.ci.provider", Some(Value::StringValue(value))) => CiProvider::MARKERS
            .into_iter()
            .any(|(_, provider)| provider.as_str() == value),
        ("gen_ai.agent.name", Some(Value::StringValue(value))) => AGENT_ENV_VARS
            .iter()
            .filter_map(|marker| AgentCaller::from_marker(marker))
            .any(|caller| caller.as_str() == value),
        ("cx.cli.query.language", Some(Value::StringValue(value))) => {
            matches!(value.as_str(), "dataprime" | "promql")
        }
        ("error.type", Some(Value::StringValue(value))) => matches!(
            value.as_str(),
            "argument_parse"
                | "configuration"
                | "profile_selection"
                | "authentication"
                | "permission"
                | "api"
                | "invalid_argument"
                | "timeout"
                | "connection"
                | "http"
                | "json"
                | "io"
                | "query_stream"
                | "cancelled"
                | "error"
        ),
        _ => false,
    }
}

fn http_method(value: &str) -> bool {
    matches!(
        value,
        "CONNECT" | "DELETE" | "GET" | "HEAD" | "OPTIONS" | "PATCH" | "POST" | "PUT" | "TRACE"
    )
}

fn http_attribute(attribute: &KeyValue) -> bool {
    let (key, value) = field(attribute);
    match (key, value) {
        ("http.request.method", Some(Value::StringValue(value))) => {
            value == "_OTHER" || http_method(value)
        }
        ("http.response.status_code", Some(Value::IntValue(value))) => (100..=999).contains(value),
        ("server.address", Some(Value::StringValue(value))) => {
            url::Url::parse(&format!("https://{value}"))
                .is_ok_and(|url| super::exportable_api_host(&url) == Some(value))
        }
        ("server.port", Some(Value::IntValue(value))) => u16::try_from(*value).is_ok(),
        ("url.template", Some(Value::StringValue(value))) => {
            HTTP_TEMPLATES.contains(&value.as_str())
        }
        ("cx.cli.region", Some(Value::StringValue(value))) => matches!(
            value.as_str(),
            "us1" | "us2" | "us3" | "eu1" | "eu2" | "ap1" | "ap2" | "ap3" | "stg1"
        ),
        ("error.type", Some(Value::StringValue(value))) => {
            matches!(value.as_str(), "connection" | "http" | "timeout")
                || (value.len() == 3
                    && value
                        .parse::<u16>()
                        .is_ok_and(|status| (100..=999).contains(&status)))
        }
        _ => false,
    }
}
