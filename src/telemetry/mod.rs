//! Client-side usage telemetry (spec: `contributing/telemetry-design-draft.md`).
//!
//! [`record`] is the only entry point commands need. Depending on
//! [`consent::mode`] it does nothing, prints the event (`CX_TELEMETRY=log`), or
//! appends it to the spool. It never fails and never prints anything else.
//!
//! `main` drives the per-run `cli.command` event: [`begin`] at start,
//! [`set_command`] after parsing, [`set_targets`] once profiles resolve, and
//! [`finish`] with the result, then [`spawn_sender`]. No command waits on the
//! network for telemetry: a detached `cx __telemetry-send` does the sending.

pub mod consent;
pub mod event;
pub mod send;
pub mod spool;

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use consent::Mode;
use event::{
    AuthMethod, CommandEvent, CommandFlags, CommandPath, ConfirmResult, CredentialStore,
    ErrorClass, Event, EventKind, OnboardingStep, Outcome, ParseKind, Resource, StatusClass,
    TeamId, Version, MAX_DURATION_MS,
};
use uuid::Uuid;

use crate::config::OutputFormat;
use crate::error::{CxError, Explained};
use crate::execution::ExecutionTarget;

static MODE: OnceLock<Mode> = OnceLock::new();
static NOTICE_OK: OnceLock<bool> = OnceLock::new();
static INVOCATION_ID: LazyLock<Uuid> = LazyLock::new(Uuid::new_v4);
static INVOCATION: LazyLock<Mutex<Invocation>> = LazyLock::new(Mutex::default);
/// The highest HTTP status the Coralogix API answered with in this run; 0
/// when it answered nothing.
static HIGHEST_STATUS: AtomicU16 = AtomicU16::new(0);

/// What this run did, filled in as `main` learns it.
#[derive(Default)]
struct Invocation {
    start: Option<Instant>,
    path: Option<CommandPath>,
    flags: CommandFlags,
    output_format: Option<OutputFormat>,
    profile_count: u32,
    team_id: Option<TeamId>,
    spilled: bool,
    /// The user declined a prompt; the outcome is `cancelled`.
    cancelled: bool,
}

fn invocation() -> MutexGuard<'static, Invocation> {
    INVOCATION.lock().unwrap_or_else(|e| e.into_inner())
}

/// The telemetry mode for this process, decided once.
pub fn mode() -> &'static Mode {
    MODE.get_or_init(consent::mode)
}

fn off() -> bool {
    *mode() == Mode::Off
}

/// Record one event of `kind`, happening now.
pub fn record(kind: EventKind) {
    match mode() {
        Mode::Off => {}
        Mode::Log => eprintln!("cx telemetry: {}", log_line(&build(kind))),
        Mode::On { .. } => {
            if !*NOTICE_OK.get_or_init(consent::ensure_notice_shown) {
                return;
            }
            if let Some(spool) = spool::path() {
                let _ = spool::append(&spool, &[build(kind)]);
            }
        }
    }
}

fn build(kind: EventKind) -> Event {
    let team_id = invocation().team_id.clone();
    Event::now(*INVOCATION_ID, team_id, kind)
}

/// The event with the resource attributes it would be sent with, as one flat
/// JSON object: everything that would leave the machine, nothing else.
fn log_line(event: &Event) -> String {
    let resource = Resource::current(&crate::version_cache::installation_id());
    let mut line = serde_json::to_value(resource).unwrap_or_default();
    if let (Some(line), Ok(serde_json::Value::Object(event))) =
        (line.as_object_mut(), serde_json::to_value(event))
    {
        line.extend(event);
    }
    line.to_string()
}

// ── Per-run lifecycle, driven by `main` ───────────────────────────────────────

/// Start timing the run. Records `cli.first_run` on a new install's first
/// run, and `cli.version_changed` on the first run of a new version.
pub fn begin() {
    invocation().start = Some(Instant::now());
    if off() {
        return;
    }
    if crate::version_cache::installation_id_first_run().1 {
        record(EventKind::FirstRun);
    }
    let current = env!("CARGO_PKG_VERSION");
    let mut state = crate::version_cache::VersionCheckCache::load();
    if state.last_run_version.as_deref() != Some(current) {
        let previous = state.last_run_version.replace(current.to_string());
        // Recorded only once the new version is saved, so it fires once.
        if state.save().is_ok() {
            if let Some(previous) = previous.as_deref().and_then(Version::parse) {
                record(EventKind::VersionChanged { previous });
            }
        }
    }
}

/// Remember which command runs, from names only (see
/// [`event::command_identity`]).
pub fn set_command(cmd: &clap::Command, matches: &clap::ArgMatches) {
    if off() {
        return;
    }
    let (path, flags) = event::command_identity(cmd, matches);
    let mut inv = invocation();
    inv.path = path;
    inv.flags = flags;
}

/// Remember the resolved output format and profiles. The team id is the one
/// already cached in the profile file, sent only for a single profile using
/// its own stored credentials; no request is made to get it.
pub fn set_targets(output: OutputFormat, targets: &[Arc<ExecutionTarget>]) {
    if off() {
        return;
    }
    let team_id = match targets {
        [t] if !t.cfg.credentials_overridden => crate::config::load_profile(&t.profile_name)
            .ok()
            .and_then(|p| p.cached_team_id)
            .and_then(TeamId::new),
        _ => None,
    };
    let mut inv = invocation();
    inv.output_format = Some(output);
    // The collector drops counts above 50; report 50 for more.
    inv.profile_count = u32::try_from(targets.len().min(50)).unwrap_or(50);
    inv.team_id = team_id;
}

/// The command wrote its output to a file instead of stdout.
pub fn mark_spilled() {
    invocation().spilled = true;
}

/// The user declined a prompt, so the command's outcome is `cancelled`.
pub fn mark_cancelled() {
    invocation().cancelled = true;
}

/// Record a `cli.onboarding` step.
pub fn onboarding(
    step: OnboardingStep,
    auth_method: Option<AuthMethod>,
    credential_store: Option<CredentialStore>,
) {
    record(EventKind::Onboarding {
        step,
        auth_method,
        credential_store,
    });
}

/// Record how a risky-command confirmation was resolved.
pub fn confirm(result: ConfirmResult) {
    if result == ConfirmResult::Declined {
        mark_cancelled();
    }
    record(EventKind::Confirm { result });
}

/// Record that an update notice for `latest` was shown. `latest` comes from
/// the GitHub or Homebrew API, so only a strict `MAJOR.MINOR.PATCH` is sent.
pub fn update_notice(latest: &str) {
    if let Some(latest) = Version::parse(latest) {
        record(EventKind::UpdateNotice { latest });
    }
}

/// The Coralogix API answered a request with `status` (for
/// `cx.http.status_class`). Called by `CxClient` for every response.
pub fn observe_status(status: u16) {
    HIGHEST_STATUS.fetch_max(status, Ordering::Relaxed);
}

/// Start the detached sender for the spooled events, unless one is already
/// running or the last one backed off. Costs one lock check and one spawn;
/// every failure is ignored and leaves the events in the spool.
pub fn spawn_sender() {
    if !matches!(mode(), Mode::On { .. }) {
        return;
    }
    let Some(spool) = spool::path() else {
        return;
    };
    if !spool.exists() || send::backing_off(&spool) {
        return;
    }
    // Free now: release it and let the sender take it. Two runs can both get
    // here; the second sender finds the lock held and exits.
    if send::lock(&spool).is_some() {
        crate::update_check::spawn_detached(&[send::SENDER_ARG], &[]);
    }
}

/// Record `cli.command` for the run that produced `result`. Nothing is
/// recorded when no subcommand was parsed.
pub fn finish(result: &anyhow::Result<()>) {
    if off() {
        return;
    }
    let event = {
        let inv = invocation();
        let Some(path) = inv.path.clone() else {
            return;
        };
        let (outcome, error_class, status_class) = if inv.cancelled {
            (Outcome::Cancelled, ErrorClass::None, StatusClass::None)
        } else {
            classify(result, HIGHEST_STATUS.load(Ordering::Relaxed))
        };
        CommandEvent {
            path,
            flags: inv.flags.clone(),
            outcome,
            error_class,
            status_class,
            duration_ms: inv.start.map_or(0, |s| {
                u64::try_from(s.elapsed().as_millis())
                    .unwrap_or(MAX_DURATION_MS)
                    .min(MAX_DURATION_MS)
            }),
            output_format: inv.output_format,
            profile_count: inv.profile_count,
            spilled: inv.spilled,
        }
    };
    let first_query =
        event.outcome == Outcome::Success && QUERY_PATHS.contains(&event.path.as_str());
    record(EventKind::Command(event));
    if first_query {
        record_first_query();
    }
}

/// Commands that run a telemetry query: the end of the onboarding funnel.
/// A test in `main.rs` checks each one is a real command.
pub const QUERY_PATHS: [&str; 5] = [
    "logs",
    "spans",
    "metrics query",
    "metrics query-range",
    "dataprime query",
];

fn record_first_query() {
    let mut state = crate::version_cache::VersionCheckCache::load();
    if state.first_query_succeeded {
        return;
    }
    state.first_query_succeeded = true;
    if state.save().is_ok() {
        onboarding(OnboardingStep::FirstQuerySucceeded, None, None);
    }
}

/// `highest_status` is the highest status the API answered with (0 for none).
/// The status class comes from it alone, so it is `none` for an error raised
/// before any response, and not `2xx` when one profile of a fan-out failed.
fn classify(
    result: &anyhow::Result<()>,
    highest_status: u16,
) -> (Outcome, ErrorClass, StatusClass) {
    let observed = StatusClass::from_status(highest_status);
    let Err(err) = result else {
        return (Outcome::Success, ErrorClass::None, observed);
    };
    let cancelled = err.chain().any(|e| {
        matches!(
            e.downcast_ref::<inquire::InquireError>(),
            Some(
                inquire::InquireError::OperationCanceled
                    | inquire::InquireError::OperationInterrupted
            )
        )
    });
    if cancelled {
        return (Outcome::Cancelled, ErrorClass::None, StatusClass::None);
    }
    let cx_error = err.chain().find_map(|e| {
        e.downcast_ref::<CxError>()
            .or_else(|| e.downcast_ref::<Explained>().map(|x| &x.cause))
    });
    let class = match cx_error {
        Some(CxError::Auth(_)) => ErrorClass::Auth,
        Some(CxError::Permission(_)) => ErrorClass::Permission,
        Some(CxError::Api { .. }) => ErrorClass::Api,
        Some(CxError::InvalidArgument(_)) => ErrorClass::Usage,
        Some(CxError::Timeout) => ErrorClass::Timeout,
        Some(CxError::Http(_)) => ErrorClass::Http,
        Some(CxError::Json(_)) => ErrorClass::Json,
        Some(CxError::Io(e)) => io_class(e),
        Some(CxError::QueryStream(_)) => ErrorClass::QueryStream,
        // Not a CxError: an untyped I/O or HTTP error, else a usage or
        // configuration error raised with `bail!`.
        None => {
            let class = err.chain().find_map(|e| {
                if let Some(io) = e.downcast_ref::<std::io::Error>() {
                    Some(io_class(io))
                } else {
                    e.downcast_ref::<reqwest::Error>().map(|r| {
                        if r.is_timeout() {
                            ErrorClass::Timeout
                        } else {
                            ErrorClass::Http
                        }
                    })
                }
            });
            class.unwrap_or(ErrorClass::Usage)
        }
    };
    (Outcome::Error, class, observed)
}

/// A file the user named that doesn't exist (a mistyped `--profile`, a wrong
/// path) is a usage error; other I/O errors are `io`.
fn io_class(err: &std::io::Error) -> ErrorClass {
    if err.kind() == std::io::ErrorKind::NotFound {
        ErrorClass::Usage
    } else {
        ErrorClass::Io
    }
}

/// Record `cli.parse_error` for a clap error. Help and version requests are
/// not errors and record nothing.
pub fn record_parse_error(err: &clap::Error) {
    use clap::error::ErrorKind;
    let kind = match err.kind() {
        ErrorKind::DisplayHelp
        | ErrorKind::DisplayVersion
        | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => return,
        ErrorKind::InvalidSubcommand => ParseKind::UnknownCommand,
        ErrorKind::UnknownArgument => ParseKind::UnknownFlag,
        ErrorKind::MissingRequiredArgument | ErrorKind::MissingSubcommand => ParseKind::MissingArg,
        _ => ParseKind::Other,
    };
    record(EventKind::ParseError { kind });
    // clap exits right after this, so the end of `main` is never reached.
    spawn_sender();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_results() {
        use anyhow::anyhow;
        let ok: anyhow::Result<()> = Ok(());
        assert_eq!(
            classify(&ok, 200),
            (Outcome::Success, ErrorClass::None, StatusClass::Success2xx)
        );
        assert_eq!(classify(&ok, 0).2, StatusClass::None);

        let api: anyhow::Result<()> = Err(CxError::Api {
            status: 503,
            message: "down".into(),
        })
        .map_err(|e| anyhow::Error::from(e).context("listing alerts"));
        assert_eq!(
            classify(&api, 503),
            (Outcome::Error, ErrorClass::Api, StatusClass::Server5xx)
        );

        let auth: anyhow::Result<()> = Err(CxError::Auth("bad key".into()).into());
        assert_eq!(classify(&auth, 401).1, ErrorClass::Auth);
        // Raised before any request ("Invalid API key format"): no status.
        assert_eq!(classify(&auth, 0).2, StatusClass::None);
        // A fan-out that succeeded overall although one profile got a 500.
        assert_eq!(classify(&ok, 500).2, StatusClass::Server5xx);

        let explained: anyhow::Result<()> = Err(Explained {
            message: "authentication failed — bad key".into(),
            cause: CxError::Auth("bad key".into()),
        }
        .into());
        assert_eq!(
            classify(&explained, 401),
            (Outcome::Error, ErrorClass::Auth, StatusClass::Client4xx)
        );

        let io: anyhow::Result<()> =
            Err(anyhow::Error::from(std::io::Error::other("disk"))
                .context("writing the result file"));
        assert_eq!(classify(&io, 0).1, ErrorClass::Io);

        let missing: anyhow::Result<()> = Err(anyhow::Error::from(std::io::Error::from(
            std::io::ErrorKind::NotFound,
        ))
        .context("Profile 'nope' not found"));
        assert_eq!(classify(&missing, 0).1, ErrorClass::Usage);

        let usage: anyhow::Result<()> = Err(anyhow!("Cannot combine --profile with --api-key"));
        assert_eq!(classify(&usage, 0).1, ErrorClass::Usage);

        let cancelled: anyhow::Result<()> = Err(inquire::InquireError::OperationCanceled.into());
        assert_eq!(classify(&cancelled, 0).0, Outcome::Cancelled);
    }
}
