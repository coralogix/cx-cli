//! Client-side usage telemetry (spec: `contributing/telemetry-design-draft.md`).
//!
//! [`record`] is the only entry point commands need. Depending on
//! [`consent::mode`] it does nothing, prints the event (`CX_TELEMETRY=log`), or
//! appends it to the spool. It never fails and never prints anything else.
//!
//! `main` drives the per-run `cli.command` event: [`begin`] at start,
//! [`set_command`] after parsing, [`set_targets`] once profiles resolve, and
//! [`finish`] with the result.

pub mod consent;
pub mod event;
pub mod spool;

use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use consent::Mode;
use event::{
    CommandEvent, CommandFlags, CommandPath, ErrorClass, Event, EventKind, Outcome, ParseKind,
    Resource, StatusClass, TeamId, MAX_DURATION_MS,
};
use uuid::Uuid;

use crate::config::OutputFormat;
use crate::error::CxError;
use crate::execution::ExecutionTarget;

static MODE: OnceLock<Mode> = OnceLock::new();
static NOTICE_OK: OnceLock<bool> = OnceLock::new();
static INVOCATION_ID: LazyLock<Uuid> = LazyLock::new(Uuid::new_v4);
static INVOCATION: LazyLock<Mutex<Invocation>> = LazyLock::new(Mutex::default);

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
    /// The command talked to the Coralogix API, so it may also flush.
    network: bool,
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
    let team_id = invocation().team_id;
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

/// Start timing the run. Records `cli.first_run` when this run created the
/// install id.
pub fn begin() {
    invocation().start = Some(Instant::now());
    if !off() && crate::version_cache::installation_id_created().1 {
        record(EventKind::FirstRun);
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
    inv.profile_count = u32::try_from(targets.len()).unwrap_or(u32::MAX);
    inv.team_id = team_id;
    inv.network = true;
}

/// The command wrote its output to a file instead of stdout.
pub fn mark_spilled() {
    invocation().spilled = true;
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
        let (outcome, error_class, status_class) = classify(result, inv.network);
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
    record(EventKind::Command(event));
}

fn classify(result: &anyhow::Result<()>, network: bool) -> (Outcome, ErrorClass, StatusClass) {
    let Err(err) = result else {
        let status = if network {
            StatusClass::Success2xx
        } else {
            StatusClass::None
        };
        return (Outcome::Success, ErrorClass::None, status);
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
    let (class, status) = match err.chain().find_map(|e| e.downcast_ref::<CxError>()) {
        Some(CxError::Auth(_)) => (ErrorClass::Auth, StatusClass::Client4xx),
        Some(CxError::Permission(_)) => (ErrorClass::Permission, StatusClass::Client4xx),
        Some(CxError::Api { status, .. }) => (ErrorClass::Api, StatusClass::from_status(*status)),
        Some(CxError::InvalidArgument(_)) => (ErrorClass::Usage, StatusClass::None),
        Some(CxError::Timeout) => (ErrorClass::Timeout, StatusClass::None),
        Some(CxError::Http(_)) => (ErrorClass::Http, StatusClass::None),
        Some(CxError::Json(_)) => (ErrorClass::Json, StatusClass::None),
        Some(CxError::Io(_)) => (ErrorClass::Io, StatusClass::None),
        Some(CxError::QueryStream(_)) => (ErrorClass::QueryStream, StatusClass::None),
        // Not a CxError: an untyped I/O or HTTP error, else a usage or
        // configuration error raised with `bail!`.
        None => {
            let class = err.chain().find_map(|e| {
                if e.is::<std::io::Error>() {
                    Some(ErrorClass::Io)
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
            (class.unwrap_or(ErrorClass::Usage), StatusClass::None)
        }
    };
    (Outcome::Error, class, status)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_results() {
        use anyhow::anyhow;
        let ok: anyhow::Result<()> = Ok(());
        assert_eq!(
            classify(&ok, true),
            (Outcome::Success, ErrorClass::None, StatusClass::Success2xx)
        );
        assert_eq!(classify(&ok, false).2, StatusClass::None);

        let api: anyhow::Result<()> = Err(CxError::Api {
            status: 503,
            message: "down".into(),
        })
        .map_err(|e| anyhow::Error::from(e).context("listing alerts"));
        assert_eq!(
            classify(&api, true),
            (Outcome::Error, ErrorClass::Api, StatusClass::Server5xx)
        );

        let auth: anyhow::Result<()> = Err(CxError::Auth("bad key".into()).into());
        assert_eq!(classify(&auth, true).1, ErrorClass::Auth);

        let io: anyhow::Result<()> =
            Err(anyhow::Error::from(std::io::Error::other("disk"))
                .context("writing the result file"));
        assert_eq!(classify(&io, false).1, ErrorClass::Io);

        let usage: anyhow::Result<()> = Err(anyhow!("Cannot combine --profile with --api-key"));
        assert_eq!(classify(&usage, false).1, ErrorClass::Usage);

        let cancelled: anyhow::Result<()> = Err(inquire::InquireError::OperationCanceled.into());
        assert_eq!(classify(&cancelled, false).0, Outcome::Cancelled);
    }
}
