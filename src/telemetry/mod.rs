//! Client-side usage telemetry (spec: `contributing/telemetry-design-draft.md`).
//!
//! [`record`] is the only entry point commands need. Depending on
//! [`consent::mode`] it does nothing, prints the event (`CX_TELEMETRY=log`), or
//! appends it to the spool. It never fails and never prints anything else.

pub mod consent;
pub mod event;
pub mod spool;

use std::sync::{LazyLock, OnceLock};

use consent::Mode;
use event::{Event, EventKind, Resource};
use uuid::Uuid;

static MODE: OnceLock<Mode> = OnceLock::new();
static NOTICE_OK: OnceLock<bool> = OnceLock::new();
static INVOCATION_ID: LazyLock<Uuid> = LazyLock::new(Uuid::new_v4);

/// The telemetry mode for this process, decided once.
pub fn mode() -> &'static Mode {
    MODE.get_or_init(consent::mode)
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
    Event::now(*INVOCATION_ID, None, kind)
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
