//! cx-specific policy and attributes layered over the generic CLI telemetry crate.

use std::borrow::Cow;
use std::env;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use cli_otel::{
    self, ConsentContext, ConsentSource, FieldScope, HttpMiddleware, KeyValue, Outcome, RunError,
    Termination, CLI_AGENT_NAME, CLI_CI_PROVIDER, CLI_TERMINATION_REASON, ERROR_TYPE,
    GEN_AI_AGENT_ID, HOST_ARCH, OS_TYPE, OS_VERSION, PROCESS_COMMAND_ARGS, PROCESS_EXECUTABLE_NAME,
    PROCESS_EXIT_CODE, PROCESS_PID, SERVICE_NAME, SERVICE_VERSION,
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Request, Url};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};

use crate::config::OutputFormat;
use crate::config::{self, TelemetryConsent};
use crate::error::CxError;
use crate::region::{region_from_url, RegionMatch};
use crate::safety::ConfirmationCancelled;

const APPLICATION_NAME: &str = "cx.application.name";
const SUBSYSTEM_NAME: &str = "cx.subsystem.name";
const CX_TERMINATION_REASON: &str = "cx.cli.termination.reason";
const CX_OUTPUT_FORMAT: &str = "cx.cli.output.format";
const CX_COMMAND_RISKY: &str = "cx.cli.command.risky";
const CX_QUERY_LANGUAGE: &str = "cx.cli.query.language";
const CX_PROFILE_COUNT: &str = "cx.cli.profile.count";
const CX_REGION: &str = "cx.cli.region";
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
        .with_command_failure_logs(crate::safety::env_is_truthy("CX_TELEMETRY_FAILURE_LOGS"))
        .with_parent_trace(cli_otel::ParentTrace {
            traceparent: env::var("TRACEPARENT").ok(),
            tracestate: env::var("TRACESTATE").ok(),
        });

    let command = build_command();
    let result = cli_otel::run_with_classifier(
        command,
        config,
        |matches| dispatch(matches.clone()),
        classify,
    )
    .await;

    match result {
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
    if !endpoint_configured || !config::telemetry_allowed() {
        return false;
    }
    match context.source {
        ConsentSource::ExplicitOptOut | ConsentSource::MalformedEnvironment => false,
        ConsentSource::ExplicitOptIn => true,
        ConsentSource::ImplicitDefault => config::telemetry_consent() == TelemetryConsent::Enabled,
    }
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
                | CX_TERMINATION_REASON
        ),
        FieldScope::Extension => matches!(
            key.as_str(),
            GEN_AI_AGENT_ID
                | CLI_AGENT_NAME
                | CLI_CI_PROVIDER
                | CX_OUTPUT_FORMAT
                | CX_COMMAND_RISKY
                | CX_QUERY_LANGUAGE
                | CX_PROFILE_COUNT
                | CX_REGION
                | "url.template"
                | "http.request.method"
                | "http.response.status_code"
                | "server.address"
                | "server.port"
        ),
    }
}

pub fn record_output_format(format: OutputFormat) {
    cli_otel::record_attribute(KeyValue::new(CX_OUTPUT_FORMAT, format.as_str()));
}

pub enum QueryLanguage {
    Dataprime,
    Promql,
}

pub fn record_command(risky_write: bool, query_language: Option<QueryLanguage>) {
    cli_otel::record_attribute(KeyValue::new(CX_COMMAND_RISKY, risky_write));
    if let Some(language) = query_language {
        let language = match language {
            QueryLanguage::Dataprime => "dataprime",
            QueryLanguage::Promql => "promql",
        };
        cli_otel::record_attribute(KeyValue::new(CX_QUERY_LANGUAGE, language));
    }
}

pub fn record_legacy_profiles() {
    record_command(false, None);
    record_output_format(OutputFormat::Text);
}

pub fn record_profile_count(count: usize) {
    if let Ok(count) = i64::try_from(count) {
        cli_otel::record_attribute(KeyValue::new(CX_PROFILE_COUNT, count));
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

fn classify(result: &anyhow::Result<()>) -> Outcome {
    let Err(error) = result else {
        cli_otel::record_attribute(KeyValue::new(
            CX_TERMINATION_REASON,
            termination_name(Termination::Completed),
        ));
        return Outcome::completed(0);
    };
    let (error_type, termination) = if let Some(error) = error.downcast_ref::<CxError>() {
        let termination = if matches!(error, CxError::Timeout) {
            Termination::Timeout
        } else {
            Termination::Completed
        };
        (cx_error_type(error), termination)
    } else if error.is::<ConfirmationCancelled>()
        || matches!(
            error.downcast_ref::<inquire::InquireError>(),
            Some(
                inquire::InquireError::OperationCanceled
                    | inquire::InquireError::OperationInterrupted
            )
        )
    {
        ("cancelled", Termination::Cancelled)
    } else {
        let category = match error.downcast_ref::<ReportedFailure>() {
            Some(ReportedFailure::ProfileSelection) => "profile_selection",
            Some(ReportedFailure::Configuration) => "configuration",
            None if error.is::<clap::Error>() => "argument_parse",
            None => "error",
        };
        (category, Termination::Completed)
    };
    cli_otel::record_attribute(KeyValue::new(
        CX_TERMINATION_REASON,
        termination_name(termination),
    ));
    Outcome {
        exit_code: 1,
        termination,
        error_type: Some(Cow::Borrowed(error_type)),
    }
}

fn cx_error_type(error: &CxError) -> &'static str {
    match error {
        CxError::Auth(_) => "authentication",
        CxError::Permission(_) => "permission",
        CxError::Api { .. } => "api",
        CxError::InvalidArgument(_) => "invalid_argument",
        CxError::Timeout => "timeout",
        CxError::Http(error) if error.is_connect() => "connection",
        CxError::Http(_) | CxError::HttpMiddleware(_) => "http",
        CxError::Json(_) => "json",
        CxError::Io(_) => "io",
        CxError::QueryStream(_) => "query_stream",
    }
}

fn termination_name(termination: Termination) -> &'static str {
    match termination {
        Termination::Completed => "completed",
        Termination::Cancelled => "cancelled",
        Termination::Timeout => "timeout",
        Termination::Signal => "signal",
    }
}

pub(crate) fn instrument_http(client: Client) -> ClientWithMiddleware {
    ClientBuilder::new(client)
        .with(
            HttpMiddleware::new(|request: &Request| {
                let mut attributes = request_attributes(request);
                let url = request.url();
                if let Some(host) = exportable_api_host(url) {
                    attributes.push(KeyValue::new("server.address", host.to_owned()));
                    if let Some(port) = url.port() {
                        attributes.push(KeyValue::new("server.port", i64::from(port)));
                    }
                }
                attributes
            })
            .with_server_address(false),
        )
        .build()
}

fn exportable_api_host(url: &Url) -> Option<&str> {
    let host = url.host_str()?;
    if matches!(host, "localhost" | "127.0.0.1" | "[::1]")
        || matches!(region_from_url(url.as_str()), RegionMatch::Known(region)
            if region.api_endpoint().strip_prefix("https://") == Some(host))
    {
        return Some(host);
    }
    None
}

fn request_attributes(request: &Request) -> Vec<KeyValue> {
    let mut attributes = Vec::new();
    if let RegionMatch::Known(region) = region_from_url(request.url().as_str()) {
        attributes.push(KeyValue::new(CX_REGION, region.to_string()));
    }
    if let Some(template) = HTTP_TEMPLATES
        .into_iter()
        .find(|path| *path == request.url().path())
    {
        attributes.push(KeyValue::new("url.template", template));
    }
    attributes
}

const HTTP_TEMPLATES: [&str; 4] = [
    "/api/v1/dataprime/query",
    "/metrics/api/v1/query",
    "/metrics/api/v1/query_range",
    "/identity/whoami",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_confirmation_cancellation_keeps_failure_exit_code() {
        let outcome = classify(&Err(ConfirmationCancelled.into()));
        assert_eq!(outcome.exit_code, 1);
        assert_eq!(outcome.error_type, Some(Cow::Borrowed("cancelled")));
        assert!(matches!(outcome.termination, Termination::Cancelled));
    }

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

    #[test]
    fn http_enrichment_only_emits_reviewed_templates_and_regions() {
        let request = |url: &str| Request::new(reqwest::Method::GET, url.parse().unwrap());
        let known = request("https://api.eu1.coralogix.com/metrics/api/v1/query?query=secret");
        assert_eq!(
            request_attributes(&known),
            [
                KeyValue::new(CX_REGION, "eu1"),
                KeyValue::new("url.template", "/metrics/api/v1/query"),
            ],
        );
        let unknown = request("https://localhost/metrics/api/v1/query/secret");
        assert!(request_attributes(&unknown).is_empty());
    }
}
