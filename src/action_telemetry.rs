//! Best-effort, privacy-safe action telemetry for `cx`.
//!
//! Events are sent only when `CX_TELEMETRY_URL` is configured. The URL is a
//! platform-owned authenticated ingestion endpoint; each event is authorized
//! with the same credential used for the command's Coralogix API request.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Error;
use clap::ArgMatches;
use futures::future::join_all;
use reqwest::Client;
use serde::Serialize;
use url::Url;

use crate::config::OutputFormat;
use crate::error::CxError;
use crate::execution::ExecutionTarget;
use crate::safety;

const TELEMETRY_URL_ENV: &str = "CX_TELEMETRY_URL";
const DISABLE_TELEMETRY_ENV: &str = "CX_NO_TELEMETRY";
const DEBUG_TELEMETRY_ENV: &str = "CX_DEBUG_TELEMETRY";
const DELIVERY_TIMEOUT: Duration = Duration::from_millis(100);
const CX_SKILL_NAMES: &[&str] = &[
    "coralogix-docs",
    "cx-alerts",
    "cx-cases",
    "cx-cost-optimization",
    "cx-dashboards",
    "cx-data-pipeline",
    "cx-infra",
    "cx-observability-setup",
    "cx-olly",
    "cx-platform-admin",
    "cx-slos",
    "cx-telemetry-querying",
];

/// Whether a completed CLI invocation succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcome {
    Success,
    Error,
}

/// A bounded event sent to the platform-owned telemetry ingestion endpoint.
///
/// Identity and tenant attributes are intentionally absent: the server derives
/// them from the bearer token, rather than trusting client-provided values.
#[derive(Debug, Clone, Serialize)]
pub struct ActionEvent {
    pub schema_version: u8,
    pub installation_id: String,
    pub command_path: String,
    pub command_family: String,
    pub outcome: ActionOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<&'static str>,
    pub cxsdk_version: &'static str,
    pub invoker_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_type: Option<&'static str>,
    pub skills_on_disk: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_target_count: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured_profile_count: Option<&'static str>,
    pub write_operation: bool,
    pub auto_approved: bool,
    pub duration_ms: u128,
}

#[derive(Clone)]
pub struct TelemetryClient {
    endpoint: String,
    bearer_token: String,
    auth_type: &'static str,
    client: Client,
}

impl TelemetryClient {
    fn from_target(target: &ExecutionTarget) -> Option<Self> {
        if !enabled() {
            return None;
        }

        let endpoint = std::env::var(TELEMETRY_URL_ENV)
            .ok()
            .filter(|url| !url.trim().is_empty())?;
        if !same_origin(&endpoint, &target.cfg.endpoint) {
            return None;
        }
        let client = Client::builder().timeout(DELIVERY_TIMEOUT).build().ok()?;

        Some(Self {
            endpoint,
            bearer_token: target.cfg.api_key.clone(),
            auth_type: target.cfg.auth_kind.as_str(),
            client,
        })
    }

    async fn send(&self, event: &ActionEvent) {
        let _ = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.bearer_token)
            .header("x-cx-sdk-version", sdk_version())
            .json(event)
            .send()
            .await;
    }
}

/// Tracks one invocation. It deliberately has no command arguments, profile
/// names, API keys, query text, request IDs, or raw error messages.
pub struct ActionSession {
    started: Instant,
    installation_id: String,
    command_path: String,
    command_family: String,
    output_format: Option<OutputFormat>,
    selected_target_count: Option<&'static str>,
    configured_profile_count: Option<&'static str>,
    auth_type: Option<&'static str>,
    write_operation: bool,
    auto_approved: bool,
    clients: Vec<TelemetryClient>,
}

impl ActionSession {
    pub fn from_matches(matches: &ArgMatches, yes: bool) -> Self {
        let (command_path, command_family) = command_path_from_matches(matches);
        let write_operation = matches
            .subcommand()
            .and_then(|_| safety::get_leaf_subcommand_name(matches))
            .is_some_and(|leaf| safety::is_write_verb(&leaf));

        Self {
            started: Instant::now(),
            installation_id: crate::version_cache::installation_id(),
            command_path,
            command_family,
            output_format: None,
            selected_target_count: None,
            configured_profile_count: configured_profile_count_bucket(),
            auth_type: Some("none"),
            write_operation,
            auto_approved: yes,
            clients: Vec::new(),
        }
    }

    pub fn set_output_format(&mut self, output_format: OutputFormat) {
        self.output_format = Some(output_format);
    }

    pub fn set_targets(&mut self, targets: &[std::sync::Arc<ExecutionTarget>]) {
        self.selected_target_count = Some(count_bucket(targets.len()));
        let mut auth_types = targets
            .iter()
            .map(|target| target.cfg.auth_kind.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter();
        self.auth_type = match (auth_types.next(), auth_types.next()) {
            (Some(auth_type), None) => Some(auth_type),
            _ => Some("multiple"),
        };
        self.clients = targets
            .iter()
            .filter_map(|target| TelemetryClient::from_target(target))
            .collect();
    }

    pub fn event(&self, result: &anyhow::Result<()>) -> ActionEvent {
        let (outcome, error_type, http_status) = match result {
            Ok(()) => (ActionOutcome::Success, None, None),
            Err(error) => {
                let (error_type, http_status) = original_error_type(error);
                (ActionOutcome::Error, Some(error_type), http_status)
            }
        };

        ActionEvent {
            schema_version: 1,
            installation_id: self.installation_id.clone(),
            command_path: self.command_path.clone(),
            command_family: self.command_family.clone(),
            outcome,
            error_type,
            http_status,
            output_format: self.output_format.map(|format| format.as_str()),
            cxsdk_version: sdk_version(),
            invoker_name: safety::invoker_name(),
            auth_type: self.auth_type,
            skills_on_disk: skills_on_disk(),
            selected_target_count: self.selected_target_count,
            configured_profile_count: self.configured_profile_count,
            write_operation: self.write_operation,
            auto_approved: self.auto_approved,
            duration_ms: self.started.elapsed().as_millis(),
        }
    }

    /// Deliver an event before the process exits. Delivery is optional,
    /// time-bounded, and ignores all telemetry failures.
    pub async fn finish(&self, result: &anyhow::Result<()>) {
        let event = self.event(result);
        if safety::env_is_truthy(DEBUG_TELEMETRY_ENV) {
            if let Ok(json) = serde_json::to_string_pretty(&event) {
                eprintln!("[cx telemetry]\n{json}");
            }
        }

        if self.clients.is_empty() {
            return;
        }

        join_all(self.clients.iter().map(|client| {
            let mut event = event.clone();
            event.auth_type = Some(client.auth_type);
            async move {
                client.send(&event).await;
            }
        }))
        .await;
    }
}

pub fn enabled() -> bool {
    !safety::env_is_truthy(DISABLE_TELEMETRY_ENV)
}

pub fn count_bucket(count: usize) -> &'static str {
    match count {
        0 => "0",
        1 => "1",
        2 => "2",
        _ => "3_plus",
    }
}

pub fn command_path_from_matches(matches: &ArgMatches) -> (String, String) {
    let mut names = Vec::new();
    let mut current = matches;
    while let Some((name, subcommand)) = current.subcommand() {
        names.push(name);
        current = subcommand;
    }

    match names.split_first() {
        Some((family, _)) => (names.join("."), (*family).to_string()),
        None => ("unknown".to_string(), "unknown".to_string()),
    }
}

/// Returns the original bounded error type without inspecting error messages.
///
/// Most command handlers preserve `CxError` inside `anyhow::Error`, allowing
/// telemetry to identify the original error variant while never serializing a
/// potentially sensitive error message.
pub fn original_error_type(error: &Error) -> (&'static str, Option<u16>) {
    if let Some(cx_error) = error.downcast_ref::<CxError>() {
        return match cx_error {
            CxError::Auth(_) => ("CxError::Auth", None),
            CxError::Permission(_) => ("CxError::Permission", None),
            CxError::Api { status, .. } => ("CxError::Api", Some(*status)),
            CxError::Http(_) => ("CxError::Http", None),
            CxError::Json(_) => ("CxError::Json", None),
            CxError::Io(_) => ("CxError::Io", None),
            CxError::QueryStream(_) => ("CxError::QueryStream", None),
        };
    }

    ("anyhow", None)
}

fn configured_profile_count_bucket() -> Option<&'static str> {
    crate::config::list_profile_names()
        .ok()
        .map(|profiles| count_bucket(profiles.len()))
}

pub const fn sdk_version() -> &'static str {
    concat!("cx-cli-", env!("CARGO_PKG_VERSION"))
}

/// Checks standard skill-installation locations for a known cx skill.
///
/// This intentionally avoids recursively scanning home directories and does
/// not count this repository's source `skills/` directory as an installation.
pub fn skills_on_disk() -> bool {
    skill_roots().iter().any(|root| contains_cx_skill(root))
}

fn skill_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.extend([
            home.join(".agents/skills"),
            home.join(".claude/skills"),
            home.join(".cursor/skills"),
        ]);
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.extend([
            cwd.join(".agents/skills"),
            cwd.join(".claude/skills"),
            cwd.join(".cursor/skills"),
        ]);
    }
    roots
}

fn contains_cx_skill(root: &Path) -> bool {
    CX_SKILL_NAMES
        .iter()
        .any(|name| root.join(name).join("SKILL.md").is_file())
}

fn same_origin(telemetry_url: &str, api_endpoint: &str) -> bool {
    let Ok(telemetry_url) = Url::parse(telemetry_url) else {
        return false;
    };
    let Ok(api_endpoint) = Url::parse(api_endpoint) else {
        return false;
    };
    telemetry_url.origin() == api_endpoint.origin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(clap::Subcommand)]
    enum TestCommand {
        Dashboards {
            #[command(subcommand)]
            command: DashboardCommand,
        },
        Logs,
    }

    #[derive(clap::Subcommand)]
    enum DashboardCommand {
        Search,
    }

    #[test]
    fn count_bucket_is_bounded() {
        assert_eq!(count_bucket(0), "0");
        assert_eq!(count_bucket(1), "1");
        assert_eq!(count_bucket(2), "2");
        assert_eq!(count_bucket(3), "3_plus");
        assert_eq!(count_bucket(99), "3_plus");
    }

    #[test]
    fn command_path_uses_full_subcommand_path() {
        let matches = TestCli::command()
            .try_get_matches_from(["cx", "dashboards", "search"])
            .unwrap();
        assert_eq!(
            command_path_from_matches(&matches),
            ("dashboards.search".to_string(), "dashboards".to_string())
        );
    }

    #[test]
    fn command_path_keeps_top_level_commands() {
        let matches = TestCli::command()
            .try_get_matches_from(["cx", "logs"])
            .unwrap();
        assert_eq!(
            command_path_from_matches(&matches),
            ("logs".to_string(), "logs".to_string())
        );
    }

    #[test]
    fn preserves_cx_error_type_without_exposing_messages() {
        let error = Error::new(CxError::Api {
            status: 429,
            message: "contains a secret that must not be emitted".to_string(),
        });
        assert_eq!(original_error_type(&error), ("CxError::Api", Some(429)));
    }

    #[test]
    fn event_json_excludes_sensitive_fields() {
        let event = ActionEvent {
            schema_version: 1,
            installation_id: "09a2b101-693f-47d2-8d7a-00e9a021855c".into(),
            command_path: "logs".into(),
            command_family: "logs".into(),
            outcome: ActionOutcome::Success,
            error_type: None,
            http_status: None,
            output_format: Some("json"),
            cxsdk_version: sdk_version(),
            invoker_name: "human".to_string(),
            auth_type: Some("api_key"),
            skills_on_disk: false,
            selected_target_count: Some("1"),
            configured_profile_count: Some("1"),
            write_operation: false,
            auto_approved: false,
            duration_ms: 1,
        };
        let body = serde_json::to_value(event).unwrap();
        assert_eq!(
            body["installation_id"],
            "09a2b101-693f-47d2-8d7a-00e9a021855c"
        );
        for sensitive_key in ["api_key", "query", "profile_name", "error_message"] {
            assert!(body.get(sensitive_key).is_none());
        }
    }

    #[test]
    fn telemetry_endpoint_must_share_api_origin() {
        assert!(same_origin(
            "https://api.eu2.coralogix.com/telemetry/v1/actions",
            "https://api.eu2.coralogix.com"
        ));
        assert!(!same_origin(
            "https://telemetry.example.com/events",
            "https://api.eu2.coralogix.com"
        ));
    }

    #[test]
    fn skills_detection_requires_a_known_skill_file() {
        let root = std::env::temp_dir().join(format!("cx-skills-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("cx-dashboards")).unwrap();
        assert!(!contains_cx_skill(&root));

        std::fs::write(
            root.join("cx-dashboards/SKILL.md"),
            "---\nname: cx-dashboards",
        )
        .unwrap();
        assert!(contains_cx_skill(&root));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn emitter_posts_only_bounded_event_fields() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/events"))
            .respond_with(ResponseTemplate::new(202))
            .mount(&server)
            .await;

        let client = TelemetryClient {
            endpoint: format!("{}/events", server.uri()),
            bearer_token: "test-token".to_string(),
            auth_type: "api_key",
            client: Client::new(),
        };
        let event = ActionEvent {
            schema_version: 1,
            installation_id: "09a2b101-693f-47d2-8d7a-00e9a021855c".into(),
            command_path: "dashboards.search".into(),
            command_family: "dashboards".into(),
            outcome: ActionOutcome::Success,
            error_type: None,
            http_status: None,
            output_format: Some("agents"),
            cxsdk_version: sdk_version(),
            invoker_name: "cursor".to_string(),
            auth_type: Some("api_key"),
            skills_on_disk: true,
            selected_target_count: Some("2"),
            configured_profile_count: Some("3_plus"),
            write_operation: false,
            auto_approved: false,
            duration_ms: 10,
        };

        client.send(&event).await;

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].headers.get("authorization").unwrap(),
            "Bearer test-token"
        );
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["command_path"], "dashboards.search");
        assert_eq!(
            body["installation_id"],
            "09a2b101-693f-47d2-8d7a-00e9a021855c"
        );
        assert!(body.get("api_key").is_none());
        assert!(body.get("query").is_none());
        assert!(body.get("profile_name").is_none());
    }
}
