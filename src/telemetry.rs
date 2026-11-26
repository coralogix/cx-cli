//! cx-specific policy and attributes layered over the generic CLI telemetry crate.

use std::env;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use cli_otel::{
    self, ConsentContext, ConsentSource, FieldScope, KeyValue, RunError, CLI_AGENT_NAME,
    CLI_CI_PROVIDER, CLI_TERMINATION_REASON, ERROR_TYPE, GEN_AI_AGENT_ID, HOST_ARCH, OS_TYPE,
    OS_VERSION, PROCESS_COMMAND_ARGS, PROCESS_EXECUTABLE_NAME, PROCESS_EXIT_CODE, PROCESS_PID,
    SERVICE_NAME, SERVICE_VERSION,
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

const APPLICATION_NAME: &str = "cx.application.name";
const SUBSYSTEM_NAME: &str = "cx.subsystem.name";
const CX_APPLICATION_HEADER: &str = "cx-application-name";
const CX_SUBSYSTEM_HEADER: &str = "cx-subsystem-name";
const DEFAULT_APPLICATION_NAME: &str = "cx-cli";
const DEFAULT_SUBSYSTEM_NAME: &str = "usage";

pub use cli_otel::with_current_context;

pub async fn run<F>(
    build_command: impl FnOnce() -> clap::Command,
    dispatch: impl FnOnce(clap::ArgMatches) -> F,
) -> anyhow::Result<()>
where
    F: Future<Output = anyhow::Result<()>>,
{
    let endpoint_configured = endpoint_is_configured();
    let config = cli_otel::Config::from_env("cx")
        .with_service_version(env!("CARGO_PKG_VERSION"))
        .with_timeout(Duration::from_secs(1))
        .with_consent_callback(Arc::new(move |context| {
            consent_allows(context, endpoint_configured)
        }))
        .with_allowlist(Arc::new(allow_field))
        .with_resource_attributes(default_resource_attributes())
        .with_endpoint_headers(default_ingress_headers())
        .with_parent_trace(cli_otel::ParentTrace {
            traceparent: env::var("TRACEPARENT").ok(),
            tracestate: env::var("TRACESTATE").ok(),
        });

    let command = build_command();
    match cli_otel::run(command, config, |matches| dispatch(matches.clone())).await {
        Ok(()) => Ok(()),
        Err(RunError::Parse(error)) => error.exit(),
        Err(RunError::Dispatch(error)) if error.is::<ReportedFailure>() => std::process::exit(1),
        Err(RunError::Dispatch(error)) => Err(error),
    }
}

fn endpoint_is_configured() -> bool {
    [
        "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "CX_OTLP_ENDPOINT",
    ]
    .iter()
    .any(|name| env::var_os(name).is_some())
}

fn consent_allows(context: ConsentContext, endpoint_configured: bool) -> bool {
    endpoint_configured && matches!(context.source, ConsentSource::ExplicitOptIn)
}

fn default_resource_attributes() -> Vec<KeyValue> {
    vec![
        KeyValue::new(APPLICATION_NAME, DEFAULT_APPLICATION_NAME),
        KeyValue::new(SUBSYSTEM_NAME, DEFAULT_SUBSYSTEM_NAME),
    ]
}

fn default_ingress_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static(CX_APPLICATION_HEADER),
        HeaderValue::from_static(DEFAULT_APPLICATION_NAME),
    );
    headers.insert(
        HeaderName::from_static(CX_SUBSYSTEM_HEADER),
        HeaderValue::from_static(DEFAULT_SUBSYSTEM_NAME),
    );
    headers
}

fn allow_field(scope: FieldScope, key: String, _value: cli_otel::Value) -> bool {
    match scope {
        FieldScope::Resource => matches!(
            key.as_str(),
            SERVICE_NAME
                | SERVICE_VERSION
                | OS_TYPE
                | HOST_ARCH
                | OS_VERSION
                | APPLICATION_NAME
                | SUBSYSTEM_NAME
        ),
        FieldScope::Command => matches!(
            key.as_str(),
            PROCESS_COMMAND_ARGS
                | PROCESS_EXECUTABLE_NAME
                | PROCESS_PID
                | PROCESS_EXIT_CODE
                | ERROR_TYPE
                | CLI_TERMINATION_REASON
        ),
        FieldScope::Extension => matches!(
            key.as_str(),
            GEN_AI_AGENT_ID | CLI_AGENT_NAME | CLI_CI_PROVIDER
        ),
    }
}

/// A failure whose complete diagnostic has already been printed by cx.
#[derive(Debug, thiserror::Error)]
pub enum ReportedFailure {
    #[error("Profile selection failed")]
    ProfileSelection,
    #[error("Configuration resolution failed")]
    Configuration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_contains_only_cx_and_generic_fields() {
        assert!(allow_field(
            FieldScope::Resource,
            APPLICATION_NAME.to_owned(),
            cli_otel::Value::from(DEFAULT_APPLICATION_NAME)
        ));
        assert!(allow_field(
            FieldScope::Extension,
            GEN_AI_AGENT_ID.to_owned(),
            cli_otel::Value::from("codex")
        ));
        assert!(!allow_field(
            FieldScope::Extension,
            "cx.secret".to_owned(),
            cli_otel::Value::from("no")
        ));
    }
}
