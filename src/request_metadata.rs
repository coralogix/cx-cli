//! Bounded CLI metadata attached to authenticated Coralogix API requests.
//!
//! Values are collected before the request is sent; response-specific data is
//! intentionally excluded.

use clap::ArgMatches;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::config::{AuthKind, OutputFormat, ResolvedConfig};
use crate::safety;

const HEADER_SCHEMA_VERSION: &str = "x-cx-cli-metadata-version";
const HEADER_METADATA: &str = "x-cx-cli-metadata";
const HEADER_INVOCATION_ID: &str = "x-cx-cli-invocation-id";
const HEADER_INSTALLATION_ID: &str = "x-cx-cli-installation-id";
const HEADER_COMMAND_PATH: &str = "x-cx-cli-command-path";
const HEADER_COMMAND_FAMILY: &str = "x-cx-cli-command-family";
const HEADER_OUTPUT_FORMAT: &str = "x-cx-cli-output-format";
const HEADER_INVOKER_NAME: &str = "x-cx-cli-invoker-name";
const HEADER_AUTH_TYPE: &str = "x-cx-cli-auth-type";
const HEADER_SKILLS_ON_DISK: &str = "x-cx-cli-skills-on-disk";
const HEADER_SELECTED_TARGET_COUNT: &str = "x-cx-cli-selected-target-count";
const HEADER_CONFIGURED_PROFILE_COUNT: &str = "x-cx-cli-configured-profile-count";
const HEADER_WRITE_OPERATION: &str = "x-cx-cli-write-operation";
const HEADER_AUTO_APPROVED: &str = "x-cx-cli-auto-approved";
const TELEMETRY_ENABLED_ENV: &str = "CX_TELEMETRY";

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

/// Metadata attached to every API request for a CLI invocation.
#[derive(Clone, Debug, Default)]
pub struct RequestMetadata {
    headers: HeaderMap,
}

impl RequestMetadata {
    pub fn from_invocation(
        matches: &ArgMatches,
        output_format: OutputFormat,
        targets: &[ResolvedConfig],
        auto_approved: bool,
    ) -> Self {
        if !enabled() {
            return Self::default();
        }

        let (command_path, command_family) = command_path_from_matches(matches);
        let write_operation = safety::get_leaf_subcommand_name(matches)
            .is_some_and(|leaf| safety::is_write_verb(&leaf));
        let auth_type = auth_type(targets);
        let configured_profile_count = crate::config::list_profile_names()
            .ok()
            .map(|profiles| count_bucket(profiles.len()))
            .unwrap_or("unknown");

        let invocation_id = Uuid::new_v4().to_string();
        let values = [
            (HEADER_SCHEMA_VERSION, "1".to_string()),
            (HEADER_INVOCATION_ID, invocation_id.clone()),
            (
                HEADER_INSTALLATION_ID,
                crate::version_cache::installation_id(),
            ),
            (HEADER_COMMAND_PATH, command_path),
            (HEADER_COMMAND_FAMILY, command_family),
            (HEADER_OUTPUT_FORMAT, output_format.as_str().to_string()),
            (HEADER_INVOKER_NAME, safety::invoker_name()),
            (HEADER_AUTH_TYPE, auth_type.to_string()),
            (
                HEADER_SKILLS_ON_DISK,
                if skills_on_disk() {
                    "true".to_string()
                } else {
                    "false".to_string()
                },
            ),
            (
                HEADER_SELECTED_TARGET_COUNT,
                count_bucket(targets.len()).to_string(),
            ),
            (
                HEADER_CONFIGURED_PROFILE_COUNT,
                configured_profile_count.to_string(),
            ),
            (
                HEADER_WRITE_OPERATION,
                if write_operation {
                    "true".to_string()
                } else {
                    "false".to_string()
                },
            ),
            (
                HEADER_AUTO_APPROVED,
                if auto_approved {
                    "true".to_string()
                } else {
                    "false".to_string()
                },
            ),
        ];

        let metadata = values
            .iter()
            .map(|(name, value)| ((*name).to_string(), sanitize(value)))
            .collect::<BTreeMap<_, _>>();
        let mut headers = HeaderMap::new();
        for (name, value) in &values {
            insert_with_limit(&mut headers, name, value, 128);
        }
        if let Ok(metadata) = serde_json::to_string(&metadata) {
            insert_with_limit(&mut headers, HEADER_METADATA, &metadata, 2_048);
        }

        Self { headers }
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}

/// Whether CLI request metadata is enabled.
///
/// Metadata is enabled by default. Set `CX_TELEMETRY=false` to opt out.
pub fn enabled() -> bool {
    !std::env::var(TELEMETRY_ENABLED_ENV)
        .ok()
        .is_some_and(|value| is_false_value(&value))
}

fn is_false_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

fn insert_with_limit(headers: &mut HeaderMap, name: &'static str, value: &str, limit: usize) {
    let value = value
        .chars()
        .filter(|character| character.is_ascii_graphic() || *character == ' ')
        .take(limit)
        .collect::<String>();
    if let Ok(value) = HeaderValue::from_str(&value) {
        headers.insert(HeaderName::from_static(name), value);
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_graphic() || *character == ' ')
        .take(128)
        .collect()
}

fn count_bucket(count: usize) -> &'static str {
    match count {
        0 => "0",
        1 => "1",
        2 => "2",
        _ => "3_plus",
    }
}

fn auth_type(targets: &[ResolvedConfig]) -> &'static str {
    let mut auth_types = targets
        .iter()
        .map(|target| target.auth_kind)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter();
    match (auth_types.next(), auth_types.next()) {
        (Some(AuthKind::ApiKey), None) => "api_key",
        (Some(AuthKind::OAuth), None) => "oauth",
        (None, _) => "none",
        _ => "multiple",
    }
}

fn command_path_from_matches(matches: &ArgMatches) -> (String, String) {
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

fn skills_on_disk() -> bool {
    skill_roots().iter().any(|root| {
        CX_SKILL_NAMES
            .iter()
            .any(|name| root.join(name).join("SKILL.md").is_file())
    })
}

fn skill_roots() -> Vec<std::path::PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser, Subcommand};

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(Subcommand)]
    enum TestCommand {
        Dashboards {
            #[command(subcommand)]
            command: DashboardCommand,
        },
    }

    #[derive(Subcommand)]
    enum DashboardCommand {
        Search,
    }

    #[test]
    fn metadata_sends_current_invocation_fields() {
        let matches = TestCli::command()
            .try_get_matches_from(["cx", "dashboards", "search"])
            .unwrap();
        let metadata = RequestMetadata::from_invocation(&matches, OutputFormat::Json, &[], false);
        let headers = metadata.headers();

        assert_eq!(headers[HEADER_SCHEMA_VERSION], "1");
        assert_eq!(headers[HEADER_COMMAND_PATH], "dashboards.search");
        assert_eq!(headers[HEADER_COMMAND_FAMILY], "dashboards");
        assert_eq!(headers[HEADER_OUTPUT_FORMAT], "json");
        assert_eq!(headers[HEADER_AUTH_TYPE], "none");
        assert_eq!(headers[HEADER_SELECTED_TARGET_COUNT], "0");
        assert_eq!(headers[HEADER_AUTO_APPROVED], "false");
        assert!(!headers[HEADER_INSTALLATION_ID].is_empty());

        let combined: serde_json::Value =
            serde_json::from_str(headers[HEADER_METADATA].to_str().unwrap()).unwrap();
        assert_eq!(
            combined[HEADER_INVOCATION_ID],
            headers[HEADER_INVOCATION_ID].to_str().unwrap()
        );
    }

    #[test]
    fn header_values_strip_control_characters() {
        let mut headers = HeaderMap::new();
        insert_with_limit(&mut headers, HEADER_INVOKER_NAME, "agent\r\ninjected", 128);
        assert_eq!(headers[HEADER_INVOKER_NAME], "agentinjected");
    }

    #[test]
    fn recognizes_telemetry_opt_out_values() {
        for value in ["0", "false", "FALSE", "no", "off"] {
            assert!(is_false_value(value), "{value} should disable metadata");
        }
        assert!(!is_false_value("true"));
    }
}
