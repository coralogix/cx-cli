//! Telemetry event types (spec: `contributing/telemetry-design-draft.md` § 1).
//!
//! Every field is an enum, a UUID, a number, a bool, or a newtype that can only
//! be built from clap names, a fixed list or a strictly parsed version. There is
//! no free-text field, so there is nowhere for a secret to end up.
//!
//! Events serialize flat, keyed by their OTLP attribute names, so the JSON that
//! `CX_TELEMETRY=log` prints and the spool stores is what gets sent. Reading a
//! spooled line back rejects unknown enum values and any string that fails the
//! check it was built with, so an edited spool cannot carry free text; unknown
//! keys are ignored and, because events are re-encoded from these types, never
//! sent.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::OutputFormat;

/// Longest a command may run and still be reported as-is; longer runs are
/// clamped to it.
pub const MAX_DURATION_MS: u64 = 3_600_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub time_unix_nano: u64,
    #[serde(rename = "cx.invocation.id")]
    pub invocation_id: Uuid,
    #[serde(rename = "cx.agent")]
    pub agent: Agent,
    #[serde(rename = "cx.ci")]
    pub ci: bool,
    #[serde(rename = "cx.tty")]
    pub tty: bool,
    #[serde(
        rename = "cx.team.id",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub team_id: Option<TeamId>,
    #[serde(flatten)]
    pub kind: EventKind,
}

impl Event {
    /// An event of `kind` happening now, in invocation `invocation_id`.
    pub fn now(invocation_id: Uuid, team_id: Option<TeamId>, kind: EventKind) -> Self {
        Self {
            time_unix_nano: super::spool::now_nanos(),
            invocation_id,
            agent: Agent::detect(),
            ci: is_ci(),
            tty: std::io::IsTerminal::is_terminal(&std::io::stdout()),
            team_id,
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event.name")]
pub enum EventKind {
    #[serde(rename = "cli.first_run")]
    FirstRun,
    #[serde(rename = "cli.command")]
    Command(CommandEvent),
    #[serde(rename = "cli.parse_error")]
    ParseError {
        #[serde(rename = "cx.parse.kind")]
        kind: ParseKind,
    },
    #[serde(rename = "cli.onboarding")]
    Onboarding {
        #[serde(rename = "cx.onboarding.step")]
        step: OnboardingStep,
        #[serde(
            rename = "cx.auth.method",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        auth_method: Option<AuthMethod>,
        #[serde(
            rename = "cx.credential.store",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        credential_store: Option<CredentialStore>,
    },
    #[serde(rename = "cli.skills_install")]
    SkillsInstall {
        #[serde(rename = "cx.skills.scope")]
        scope: crate::commands::skills::SkillsScope,
        #[serde(rename = "cx.outcome")]
        outcome: Outcome,
    },
    #[serde(rename = "cli.update_notice")]
    UpdateNotice {
        #[serde(rename = "cx.version.latest")]
        latest: Version,
    },
    #[serde(rename = "cli.version_changed")]
    VersionChanged {
        #[serde(rename = "cx.version.previous")]
        previous: Version,
    },
    #[serde(rename = "cli.confirm")]
    Confirm {
        #[serde(rename = "cx.confirm.result")]
        result: ConfirmResult,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandEvent {
    #[serde(rename = "cx.command.path")]
    pub path: CommandPath,
    #[serde(rename = "cx.command.flags")]
    pub flags: CommandFlags,
    #[serde(rename = "cx.outcome")]
    pub outcome: Outcome,
    #[serde(rename = "cx.error.class")]
    pub error_class: ErrorClass,
    #[serde(rename = "cx.http.status_class")]
    pub status_class: StatusClass,
    #[serde(rename = "cx.duration_ms")]
    pub duration_ms: u64,
    #[serde(
        rename = "cx.output.format",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub output_format: Option<OutputFormat>,
    #[serde(rename = "cx.profile.count")]
    pub profile_count: u32,
    #[serde(rename = "cx.spilled")]
    pub spilled: bool,
}

// ── Enums ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    None,
    Auth,
    Permission,
    Api,
    Timeout,
    Http,
    Json,
    Io,
    QueryStream,
    Usage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusClass {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "2xx")]
    Success2xx,
    #[serde(rename = "4xx")]
    Client4xx,
    #[serde(rename = "5xx")]
    Server5xx,
}

impl StatusClass {
    pub fn from_status(status: u16) -> Self {
        match status {
            200..=299 => Self::Success2xx,
            400..=499 => Self::Client4xx,
            500..=599 => Self::Server5xx,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseKind {
    UnknownCommand,
    UnknownFlag,
    MissingArg,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingStep {
    InitStarted,
    InitCompleted,
    ProfileAdded,
    OauthStarted,
    OauthSucceeded,
    OauthFailed,
    FirstQuerySucceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    Oauth,
    ApiKey,
}

impl From<crate::config::AuthKind> for AuthMethod {
    fn from(kind: crate::config::AuthKind) -> Self {
        match kind {
            crate::config::AuthKind::OAuth => Self::Oauth,
            crate::config::AuthKind::ApiKey => Self::ApiKey,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStore {
    Keychain,
    File,
}

impl From<crate::config::CredentialStorage> for CredentialStore {
    fn from(storage: crate::config::CredentialStorage) -> Self {
        match storage {
            crate::config::CredentialStorage::OsStore => Self::Keychain,
            crate::config::CredentialStorage::File => Self::File,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmResult {
    Accepted,
    Declined,
    SkippedYes,
    NonInteractive,
}

// ── Constrained strings ───────────────────────────────────────────────────────

/// `[a-z0-9-]{1,max}`: the shape of every clap command and flag name.
fn is_name(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Deserializing a constrained string (reading a spool line back) accepts
/// only what `$build` would have produced from it.
macro_rules! checked_string {
    ($type:ident, $build:expr) => {
        impl TryFrom<String> for $type {
            type Error = &'static str;

            fn try_from(s: String) -> Result<Self, Self::Error> {
                let build: fn(&str) -> Option<$type> = $build;
                build(&s)
                    .filter(|built| built.0 == s)
                    .ok_or(concat!("not a valid ", stringify!($type)))
            }
        }
    };
}

/// Subcommand names joined by spaces, e.g. `alerts list`. At most 4 words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct CommandPath(String);
checked_string!(CommandPath, |s| CommandPath::from_names(s.split(' ')));

impl CommandPath {
    const MAX_WORDS: usize = 4;

    /// Keeps names up to the first one that is not a valid name, and at most
    /// [`Self::MAX_WORDS`]. `None` if no name is left.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let words: Vec<&str> = names
            .into_iter()
            .take_while(|n| is_name(n, 32))
            .take(Self::MAX_WORDS)
            .collect();
        (!words.is_empty()).then(|| Self(words.join(" ")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Sorted, de-duplicated `--long` flag names joined by commas, e.g.
/// `--limit,--start`. Names only, never values. Empty when no flag was given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct CommandFlags(String);
checked_string!(CommandFlags, |s| Some(CommandFlags::from_long_names(
    s.split(',').filter_map(|flag| flag.strip_prefix("--"))
)));

impl CommandFlags {
    const MAX_FLAGS: usize = 30;

    /// Takes long names without the leading `--`. Invalid names are skipped.
    pub fn from_long_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let names: std::collections::BTreeSet<&str> =
            names.into_iter().filter(|n| is_name(n, 40)).collect();
        let flags: Vec<String> = names
            .into_iter()
            .take(Self::MAX_FLAGS)
            .map(|n| format!("--{n}"))
            .collect();
        Self(flags.join(","))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A value no command line can produce, for size-limit tests.
    #[cfg(test)]
    pub(crate) fn unchecked(flags: String) -> Self {
        Self(flags)
    }
}

/// The subcommand path and the long flag names given on the command line, read
/// from names only: no accessor that returns an argument's value is called.
/// `cmd` is the tree `matches` was parsed with (a fresh, unbuilt copy is fine).
/// Flags set from env vars or defaults are not included. The path is `None`
/// when no subcommand was given.
pub fn command_identity(
    cmd: &clap::Command,
    matches: &clap::ArgMatches,
) -> (Option<CommandPath>, CommandFlags) {
    use clap::parser::ValueSource;

    let mut names = Vec::new();
    let mut flags = Vec::new();
    let (mut cmd, mut matches) = (cmd, matches);
    loop {
        // Global args given after a subcommand are propagated up to the root's
        // matches, so the root level sees them too; duplicates are collapsed.
        flags.extend(
            cmd.get_arguments()
                .filter(|arg| {
                    matches.value_source(arg.get_id().as_str()) == Some(ValueSource::CommandLine)
                })
                .filter_map(|arg| arg.get_long()),
        );
        let Some((name, sub_matches)) = matches.subcommand() else {
            break;
        };
        let Some(sub_cmd) = cmd.find_subcommand(name) else {
            break;
        };
        names.push(name);
        (cmd, matches) = (sub_cmd, sub_matches);
    }
    (
        CommandPath::from_names(names),
        CommandFlags::from_long_names(flags),
    )
}

/// A strict `MAJOR.MINOR.PATCH` version. Versions can come from the GitHub
/// releases API, so they are parsed, never copied through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Version(String);
checked_string!(Version, Version::parse);

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        let max_len = [3, 3, 4];
        let valid = parts.len() == 3
            && parts.iter().zip(max_len).all(|(p, max)| {
                !p.is_empty() && p.len() <= max && p.bytes().all(|b| b.is_ascii_digit())
            });
        valid.then(|| Self(s.to_string()))
    }
}

/// The team id a profile has cached. Sent as a digit string. Claimed by the
/// client, not verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct TeamId(String);
checked_string!(TeamId, |s| s.parse().ok().and_then(TeamId::new));

impl TeamId {
    pub fn new(id: i64) -> Option<Self> {
        (id > 0).then(|| Self(id.to_string()))
    }
}

/// `direct`, or the lowercased name of the first agent env var that is set
/// (`safety::AGENT_ENV_VARS`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Agent(String);
checked_string!(Agent, |s| Agent::names().find(|name| name == s).map(Agent));

impl Agent {
    /// Every value `cx.agent` can have.
    fn names() -> impl Iterator<Item = String> {
        std::iter::once("direct".to_string()).chain(
            crate::safety::AGENT_ENV_VARS
                .iter()
                .map(|var| var.to_ascii_lowercase()),
        )
    }

    pub fn detect() -> Self {
        let name = crate::safety::AGENT_ENV_VARS
            .iter()
            .find(|var| std::env::var_os(var).is_some())
            .map_or_else(|| "direct".to_string(), |var| var.to_ascii_lowercase());
        Self(name)
    }
}

/// True on common CI systems. `CI` covers GitHub Actions, GitLab, CircleCI,
/// Travis, Buildkite and most others; the rest name the few that don't set it.
fn is_ci() -> bool {
    let ci = std::env::var("CI")
        .is_ok_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"));
    ci || ["JENKINS_URL", "TF_BUILD", "CODEBUILD_BUILD_ID"]
        .iter()
        .any(|var| std::env::var_os(var).is_some())
}

// ── Resource ──────────────────────────────────────────────────────────────────

/// Attributes shared by every event in a request (OTLP resource attributes).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resource {
    /// The application and subsystem the logs land under in the telemetry
    /// team; Coralogix ingestion needs both.
    #[serde(rename = "cx.application.name")]
    pub application_name: &'static str,
    #[serde(rename = "cx.subsystem.name")]
    pub subsystem_name: &'static str,
    #[serde(rename = "service.name")]
    pub service_name: &'static str,
    #[serde(rename = "service.version")]
    pub service_version: &'static str,
    #[serde(rename = "os.type")]
    pub os_type: &'static str,
    #[serde(rename = "host.arch")]
    pub host_arch: &'static str,
    #[serde(rename = "cx.install.id")]
    pub install_id: Uuid,
    #[serde(rename = "cx.install.method")]
    pub install_method: &'static str,
}

impl Resource {
    pub fn current(install_id: &str) -> Self {
        use crate::install_method::InstallMethod;
        Self {
            application_name: "cx-cli",
            subsystem_name: "usage",
            service_name: "cx-cli",
            service_version: env!("CARGO_PKG_VERSION"),
            // Values outside the spec's lists pass through; there is nothing
            // better to send.
            os_type: match std::env::consts::OS {
                "macos" => "darwin",
                other => other,
            },
            host_arch: match std::env::consts::ARCH {
                "aarch64" => "arm64",
                "x86_64" => "amd64",
                other => other,
            },
            // An edited state file can hold anything: send the nil UUID
            // rather than copy free text through.
            install_id: Uuid::parse_str(install_id).unwrap_or(Uuid::nil()),
            install_method: match crate::install_method::detect() {
                InstallMethod::Brew => "brew",
                InstallMethod::Curl => "curl",
                InstallMethod::Unknown => "unknown",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn command_event() -> Event {
        Event {
            time_unix_nano: 1_794_480_000_000_000_000,
            invocation_id: Uuid::parse_str("b1d1c0de-2f43-4a0b-9d0e-6c1f5e7a8b90").unwrap(),
            agent: Agent("claude_code".into()),
            ci: false,
            tty: false,
            team_id: None,
            kind: EventKind::Command(CommandEvent {
                path: CommandPath::from_names(["logs"]).unwrap(),
                flags: CommandFlags::from_long_names(["start", "limit"]),
                outcome: Outcome::Success,
                error_class: ErrorClass::None,
                status_class: StatusClass::Success2xx,
                duration_ms: 412,
                output_format: Some(OutputFormat::Toon),
                profile_count: 1,
                spilled: false,
            }),
        }
    }

    #[test]
    fn command_event_serializes_flat_with_spec_keys() {
        let value = serde_json::to_value(command_event()).unwrap();
        assert_eq!(
            value,
            json!({
                "time_unix_nano": 1_794_480_000_000_000_000u64,
                "event.name": "cli.command",
                "cx.invocation.id": "b1d1c0de-2f43-4a0b-9d0e-6c1f5e7a8b90",
                "cx.agent": "claude_code",
                "cx.ci": false,
                "cx.tty": false,
                "cx.command.path": "logs",
                "cx.command.flags": "--limit,--start",
                "cx.outcome": "success",
                "cx.error.class": "none",
                "cx.http.status_class": "2xx",
                "cx.duration_ms": 412,
                "cx.output.format": "toon",
                "cx.profile.count": 1,
                "cx.spilled": false
            })
        );
    }

    #[test]
    fn every_event_kind_round_trips() {
        let kinds = vec![
            EventKind::FirstRun,
            command_event().kind,
            EventKind::ParseError {
                kind: ParseKind::UnknownFlag,
            },
            EventKind::Onboarding {
                step: OnboardingStep::OauthSucceeded,
                auth_method: Some(AuthMethod::Oauth),
                credential_store: Some(CredentialStore::Keychain),
            },
            EventKind::SkillsInstall {
                scope: crate::commands::skills::SkillsScope::Global,
                outcome: Outcome::Success,
            },
            EventKind::UpdateNotice {
                latest: Version::parse("0.2.0").unwrap(),
            },
            EventKind::VersionChanged {
                previous: Version::parse("0.1.32").unwrap(),
            },
            EventKind::Confirm {
                result: ConfirmResult::SkippedYes,
            },
        ];
        for kind in kinds {
            let event = Event {
                team_id: TeamId::new(4015532),
                kind,
                ..command_event()
            };
            let line = serde_json::to_string(&event).unwrap();
            let back: Event = serde_json::from_str(&line).unwrap();
            assert_eq!(back, event, "round trip failed for {line}");
        }
    }

    #[test]
    fn enums_serialize_to_spec_strings() {
        let s = |v: Value| v.as_str().unwrap().to_string();
        assert_eq!(s(json!(ErrorClass::QueryStream)), "query_stream");
        assert_eq!(s(json!(StatusClass::Client4xx)), "4xx");
        assert_eq!(s(json!(ParseKind::UnknownCommand)), "unknown_command");
        assert_eq!(
            s(json!(OnboardingStep::FirstQuerySucceeded)),
            "first_query_succeeded"
        );
        assert_eq!(s(json!(AuthMethod::ApiKey)), "api_key");
        assert_eq!(s(json!(ConfirmResult::NonInteractive)), "non_interactive");
        assert_eq!(s(json!(TeamId::new(42).unwrap())), "42");
    }

    #[test]
    fn unknown_values_are_rejected_and_unknown_keys_dropped() {
        let mut value = serde_json::to_value(command_event()).unwrap();
        value["cx.outcome"] = json!("pwned");
        assert!(serde_json::from_value::<Event>(value).is_err());

        let mut value = serde_json::to_value(command_event()).unwrap();
        value["event.name"] = json!("cli.evil");
        assert!(serde_json::from_value::<Event>(value).is_err());

        let mut value = serde_json::to_value(command_event()).unwrap();
        value["cx.query"] = json!("source logs | filter secret");
        let event: Event = serde_json::from_value(value).unwrap();
        let line = serde_json::to_string(&event).unwrap();
        assert!(!line.contains("cx.query") && !line.contains("secret"));
    }

    /// A hand-edited spool line: each constrained string is checked again
    /// when the line is read back.
    #[test]
    fn edited_spool_values_are_rejected() {
        for (key, bad) in [
            ("cx.command.path", json!("logs; rm -rf /")),
            ("cx.command.path", json!("a b c d e")),
            ("cx.command.path", json!("")),
            ("cx.command.flags", json!("--limit,free text")),
            ("cx.command.flags", json!("limit")),
            ("cx.command.flags", json!("--b,--a")),
            ("cx.agent", json!("someone@example.com")),
            ("cx.agent", json!("CLAUDE_CODE")),
            ("cx.team.id", json!("my-team")),
            ("cx.team.id", json!("007")),
            ("cx.team.id", json!("-5")),
        ] {
            let mut value = serde_json::to_value(command_event()).unwrap();
            value[key] = bad.clone();
            let read = serde_json::from_value::<Event>(value);
            assert!(read.is_err(), "{key} = {bad} was accepted");
        }
        let version = |v: &str| {
            serde_json::from_value::<EventKind>(
                json!({ "event.name": "cli.update_notice", "cx.version.latest": v }),
            )
        };
        assert!(version("0.2.0").is_ok());
        assert!(version("0.2.0-beta; drop").is_err());
    }

    #[test]
    fn constrained_strings() {
        assert_eq!(
            CommandPath::from_names(["alerts", "list"])
                .unwrap()
                .as_str(),
            "alerts list"
        );
        assert_eq!(
            CommandPath::from_names(["a", "b", "c", "d", "e"])
                .unwrap()
                .as_str(),
            "a b c d"
        );
        assert!(CommandPath::from_names(["Bad"]).is_none());
        assert_eq!(CommandFlags::from_long_names([]).as_str(), "");
        assert_eq!(
            CommandFlags::from_long_names(["b", "a", "b", "NOPE"]).as_str(),
            "--a,--b"
        );
        assert!(Version::parse("1.2.3").is_some());
        for bad in ["v1.2.3", "1.2", "1.2.3-rc1", "1.2.3.4", "1234.0.0", ""] {
            assert!(Version::parse(bad).is_none(), "{bad} should be rejected");
        }
        assert!(TeamId::new(0).is_none() && TeamId::new(-5).is_none());
    }

    #[test]
    fn resource_maps_platform_names() {
        let r = Resource::current("not-a-uuid");
        assert_eq!(r.service_name, "cx-cli");
        assert!(matches!(r.os_type, "darwin" | "linux" | "windows"));
        assert!(matches!(r.host_arch, "arm64" | "amd64"));
        assert!(r.install_id.is_nil());
    }

    fn test_cli() -> clap::Command {
        use clap::{Arg, ArgAction, Command};
        Command::new("cx")
            .arg(Arg::new("profile").long("profile").global(true))
            .arg(
                Arg::new("yes")
                    .long("yes")
                    .global(true)
                    .action(ArgAction::SetTrue),
            )
            .subcommand(
                Command::new("alerts").subcommand(
                    Command::new("list")
                        .arg(Arg::new("limit").long("limit"))
                        .arg(Arg::new("order-by").long("order-by").default_value("name"))
                        .arg(Arg::new("query")),
                ),
            )
            .subcommand(Command::new("cleanup"))
    }

    fn identity(args: &[&str]) -> (Option<String>, String) {
        let matches = test_cli().try_get_matches_from(args).unwrap();
        let (path, flags) = command_identity(&test_cli(), &matches);
        (path.map(|p| p.0), flags.0)
    }

    #[test]
    fn command_identity_never_contains_values() {
        let secret = "SENTINEL-sk-live-123";
        let args = [
            "cx",
            "alerts",
            "list",
            "--limit",
            secret,
            secret,
            "--profile",
            secret,
        ];
        let matches = test_cli().try_get_matches_from(args).unwrap();
        let (path, flags) = command_identity(&test_cli(), &matches);
        let event = Event {
            kind: EventKind::Command(CommandEvent {
                path: path.unwrap(),
                flags,
                ..match command_event().kind {
                    EventKind::Command(c) => c,
                    _ => unreachable!(),
                }
            }),
            ..command_event()
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("SENTINEL"), "value leaked: {json}");
        assert!(json.contains(r#""cx.command.path":"alerts list""#));
        assert!(json.contains(r#""cx.command.flags":"--limit,--profile""#));
    }

    #[test]
    fn command_identity_paths_and_flag_order() {
        assert_eq!(
            identity(&["cx", "--yes", "alerts", "list", "--limit", "5"]),
            (Some("alerts list".into()), "--limit,--yes".into())
        );
        assert_eq!(
            identity(&["cx", "alerts", "list", "--yes", "--limit=5"]),
            (Some("alerts list".into()), "--limit,--yes".into())
        );
        // A default value is not a flag the user gave.
        assert_eq!(
            identity(&["cx", "cleanup"]),
            (Some("cleanup".into()), String::new())
        );
        assert_eq!(identity(&["cx"]), (None, String::new()));
    }
}
