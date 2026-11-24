//! cx-owned configuration and safe fields for the CLI execution trace.

use std::env;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use crate::cli_otel::{
    ClapError, Config, Delivery, HttpMiddleware, KeyValue, Outcome, ParentTrace, Telemetry,
    Termination,
};
use crate::config::OutputFormat;
use crate::error::CxError;
use crate::install_method::{self, InstallMethod};
use crate::safety::{ConfirmationCancelled, AGENT_ENV_VARS};
use reqwest::{Client, Request};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};

pub use crate::cli_otel::with_current_context;
use crate::region::{region_from_url, RegionMatch};

mod delivery;
mod key;
mod notice;
mod platform;
mod privacy;
mod sender;

use delivery::Destination;

/// Handle the private sender entry point before the normal CLI is initialized.
#[doc(hidden)]
pub fn run_sender_if_requested(build_command: impl FnOnce() -> clap::Command) -> bool {
    if !sender::requested() {
        return false;
    }
    sender::run(build_command);
    true
}

pub async fn run<F>(
    build_command: impl FnOnce() -> clap::Command,
    dispatch: impl FnOnce(clap::ArgMatches) -> F,
) -> anyhow::Result<()>
where
    F: Future<Output = anyhow::Result<()>>,
{
    let (telemetry, background) = initialize().unzip();
    let command = build_command();
    let traceparent = env::var("TRACEPARENT").ok();
    let tracestate = env::var("TRACESTATE").ok();
    let result = crate::cli_otel::run_clap(
        telemetry,
        ParentTrace {
            traceparent: traceparent.as_deref(),
            tracestate: tracestate.as_deref(),
        },
        command,
        env::args_os(),
        record_invocation_context,
        dispatch,
        classify,
    )
    .await;

    if let Some(directory) = background.flatten().filter(|_| consent_allows()) {
        sender::spawn(&directory);
    }

    match result {
        Ok(()) => Ok(()),
        Err(ClapError::Parse(error)) => error.exit(),
        Err(ClapError::Command(error)) if error.is::<ReportedFailure>() => std::process::exit(1),
        Err(ClapError::Command(error)) => Err(error),
    }
}

fn initialize() -> Option<(Telemetry, Option<PathBuf>)> {
    if !consent_allows() {
        return None;
    }

    let destination = Destination::resolve()?;
    if !notice::allow_collection_after_notice() {
        return None;
    }
    let mut config = Config::new(destination.url().clone(), "cx", env!("CARGO_PKG_VERSION"));
    config.delivery_allowed = Some(Arc::new(consent_allows as fn() -> bool));
    let background = if destination.is_background() {
        let directory = destination.spool_path()?;
        config.delivery = Delivery::Spool(crate::cli_otel::Spool::new(directory.clone()));
        Some(directory)
    } else {
        config.headers = delivery::explicit_headers()?;
        config.command_failure_logs = destination.failure_logs_enabled();
        None
    };
    config.resource_attributes =
        resource_attributes(matches!(destination, Destination::Discovery { .. }));
    Some((Telemetry::new(config)?, background))
}

fn consent_allows() -> bool {
    let configured = match env::var("CX_TELEMETRY") {
        Ok(value) => value.trim().is_empty() || truthy(&value),
        Err(env::VarError::NotPresent) => true,
        Err(env::VarError::NotUnicode(_)) => false,
    };
    configured
        && !env::var("DO_NOT_TRACK").is_ok_and(|value| truthy(&value))
        && crate::config::telemetry_allowed()
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes"
    )
}

fn resource_attributes(discovered: bool) -> Vec<KeyValue> {
    let os_type = match env::consts::OS {
        "macos" => "darwin",
        os => os,
    };
    let architecture = architecture_name(env::consts::ARCH).unwrap_or(env::consts::ARCH);

    let install_method = match install_method::detect() {
        InstallMethod::Brew => "brew",
        InstallMethod::Curl => "install_script",
        InstallMethod::Unknown => "unknown",
    };
    let mut attributes = vec![
        KeyValue::new("os.type", os_type),
        KeyValue::new("host.arch", architecture),
        KeyValue::new("cli.installation.method", install_method),
    ];
    if let Some(version) = platform::os_version() {
        attributes.push(KeyValue::new("os.version", version));
    }
    if let Ok(id) = uuid::Uuid::parse_str(&crate::version_cache::installation_id()) {
        attributes.push(KeyValue::new("app.installation.id", id.to_string()));
    }
    if discovered {
        attributes.push(KeyValue::new("cx.application.name", "cx-cli"));
        attributes.push(KeyValue::new("cx.subsystem.name", "usage"));
    }
    attributes
}

fn architecture_name(architecture: &str) -> Option<&'static str> {
    Some(match architecture {
        "aarch64" | "arm64" => "arm64",
        "x86_64" | "amd64" => "amd64",
        "i386" | "i686" | "x86" => "x86",
        "arm" | "armv7l" | "arm32" => "arm32",
        "powerpc64" | "ppc64" | "ppc64le" => "ppc64",
        "riscv64" => "riscv64",
        "s390x" => "s390x",
        _ => return None,
    })
}

enum CiProvider {
    GitHubActions,
    GitLab,
    CircleCi,
    AzurePipelines,
    Buildkite,
}

impl CiProvider {
    const MARKERS: [(&str, Self); 5] = [
        ("GITHUB_ACTIONS", Self::GitHubActions),
        ("GITLAB_CI", Self::GitLab),
        ("CIRCLECI", Self::CircleCi),
        ("TF_BUILD", Self::AzurePipelines),
        ("BUILDKITE", Self::Buildkite),
    ];

    fn from_env() -> Option<Self> {
        Self::MARKERS.into_iter().find_map(|(variable, provider)| {
            env::var(variable)
                .is_ok_and(|value| value.trim().eq_ignore_ascii_case("true"))
                .then_some(provider)
        })
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::GitHubActions => "github_actions",
            Self::GitLab => "gitlab_ci",
            Self::CircleCi => "circleci",
            Self::AzurePipelines => "azure_pipelines",
            Self::Buildkite => "buildkite",
        }
    }
}

enum AgentCaller {
    Aider,
    AmazonQ,
    AwsQDeveloper,
    ClaudeCode,
    Cline,
    Codex,
    GitHubCopilot,
    CursorAgent,
    GeminiCodeAssist,
    SrcCody,
    WindsurfAgent,
}

impl AgentCaller {
    fn from_env() -> Option<Self> {
        AGENT_ENV_VARS.iter().find_map(|variable| {
            env::var(variable).ok()?;
            Self::from_marker(variable)
        })
    }

    fn from_marker(variable: &str) -> Option<Self> {
        Some(match variable {
            "AIDER" => Self::Aider,
            "AMAZON_Q" => Self::AmazonQ,
            "AWS_Q_DEVELOPER" => Self::AwsQDeveloper,
            "CLAUDECODE" | "CLAUDE_CODE" => Self::ClaudeCode,
            "CLINE" => Self::Cline,
            "CODEX" | "OPENAI_CODEX" => Self::Codex,
            "COPILOT_AGENT" | "GITHUB_COPILOT" => Self::GitHubCopilot,
            "CURSOR_AGENT" => Self::CursorAgent,
            "GEMINI_CODE_ASSIST" => Self::GeminiCodeAssist,
            "SRC_CODY" => Self::SrcCody,
            "WINDSURF_AGENT" => Self::WindsurfAgent,
            _ => return None,
        })
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Aider => "aider",
            Self::AmazonQ => "amazon-q",
            Self::AwsQDeveloper => "aws-q-developer",
            Self::ClaudeCode => "claude-code",
            Self::Cline => "cline",
            Self::Codex => "codex",
            Self::GitHubCopilot => "github-copilot",
            Self::CursorAgent => "cursor-agent",
            Self::GeminiCodeAssist => "gemini-code-assist",
            Self::SrcCody => "src-cody",
            Self::WindsurfAgent => "windsurf-agent",
        }
    }
}

fn record_invocation_context() {
    if let Some(provider) = CiProvider::from_env() {
        crate::cli_otel::record_attribute(KeyValue::new("cli.ci.provider", provider.as_str()));
    }
    if let Some(caller) = AgentCaller::from_env() {
        crate::cli_otel::record_attribute(KeyValue::new("gen_ai.agent.name", caller.as_str()));
    }
}

pub fn record_output_format(format: OutputFormat) {
    crate::cli_otel::record_attribute(KeyValue::new("cli.output.format", format.as_str()));
}

pub enum QueryLanguage {
    Dataprime,
    Promql,
}

pub fn record_command(risky_write: bool, query_language: Option<QueryLanguage>) {
    crate::cli_otel::record_attribute(KeyValue::new("cx.cli.command.risky", risky_write));
    if let Some(language) = query_language {
        let language = match language {
            QueryLanguage::Dataprime => "dataprime",
            QueryLanguage::Promql => "promql",
        };
        crate::cli_otel::record_attribute(KeyValue::new("cx.cli.query.language", language));
    }
}

pub fn record_legacy_profiles() {
    record_command(false, None);
    record_output_format(OutputFormat::Text);
}

pub fn record_profile_count(count: usize) {
    if let Ok(count) = i64::try_from(count) {
        crate::cli_otel::record_attribute(KeyValue::new("cx.cli.profile.count", count));
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
    Outcome {
        exit_code: 1,
        termination,
        error_type: Some(error_type),
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

fn exportable_api_host(url: &url::Url) -> Option<&str> {
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
        attributes.push(KeyValue::new("cx.cli.region", region.to_string()));
    }
    // Export reviewed static routes only. Other paths can include resource
    // identifiers or query values.
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
        assert_eq!(outcome.error_type, Some("cancelled"));
        assert!(matches!(outcome.termination, Termination::Cancelled));
    }

    #[test]
    fn http_enrichment_only_emits_reviewed_templates_and_regions() {
        let request = |url: &str| Request::new(reqwest::Method::GET, url.parse().unwrap());
        let known = request("https://api.eu1.coralogix.com/metrics/api/v1/query?query=secret");
        assert_eq!(
            request_attributes(&known),
            [
                KeyValue::new("cx.cli.region", "eu1"),
                KeyValue::new("url.template", "/metrics/api/v1/query"),
            ],
        );
        let unknown = request("https://localhost/metrics/api/v1/query/secret");
        assert!(request_attributes(&unknown).is_empty());
    }
}
