//! Whether telemetry runs, and the one-time notice printed before the first
//! event is recorded.

use crate::version_cache::VersionCheckCache;

/// The collector URL baked into release builds. Source and dev builds have
/// none, so telemetry is off for them unless `CX_TELEMETRY_ENDPOINT` is set at
/// runtime (tests, staging).
const BUILD_ENDPOINT: Option<&str> = option_env!("CX_TELEMETRY_ENDPOINT");

pub const NOTICE: &str = "cx collects usage data (command names, versions, error categories \
— never queries, arguments, credentials, or personal data) to improve the CLI.\n\
Details: https://github.com/coralogix/cx-cli/blob/master/docs/telemetry.md \
· Opt out: CX_TELEMETRY=0 or `telemetry = false` in ~/.cx/config.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// No event is built.
    Off,
    /// Each event is printed to stderr; nothing is stored or sent.
    Log,
    /// Events are spooled and sent to `endpoint`.
    On { endpoint: String },
}

/// The mode for this process, from the env vars, `~/.cx/config.toml` and the
/// endpoint.
pub fn mode() -> Mode {
    let env = |var| std::env::var(var).ok();
    decide(
        env("CX_TELEMETRY").as_deref(),
        env("DO_NOT_TRACK").as_deref(),
        crate::config::load_config().map_or(true, |c| c.telemetry),
        env("CX_TELEMETRY_ENDPOINT").or(BUILD_ENDPOINT.map(str::to_string)),
    )
}

/// `CX_TELEMETRY` is the most specific switch, so it wins either way; then any
/// opt-out; then telemetry needs somewhere to send to.
fn decide(
    cx_telemetry: Option<&str>,
    do_not_track: Option<&str>,
    config_enabled: bool,
    endpoint: Option<String>,
) -> Mode {
    let lower = |v: Option<&str>| v.map(|v| v.trim().to_ascii_lowercase());
    match lower(cx_telemetry).as_deref() {
        Some("0" | "false" | "off" | "no") => return Mode::Off,
        Some("log") => return Mode::Log,
        _ => {}
    }
    if matches!(
        lower(do_not_track).as_deref(),
        Some("1" | "true" | "yes" | "on")
    ) || !config_enabled
    {
        return Mode::Off;
    }
    match endpoint.filter(|e| !e.trim().is_empty()) {
        Some(endpoint) => Mode::On { endpoint },
        None => Mode::Off,
    }
}

/// Print the notice once, on stderr. Returns whether recording may go ahead:
/// false when the notice cannot be marked as shown, so no event is ever
/// recorded without the user having seen it.
pub fn ensure_notice_shown() -> bool {
    let mut state = VersionCheckCache::load();
    if state.telemetry_notice_shown {
        return true;
    }
    eprintln!("{NOTICE}");
    state.telemetry_notice_shown = true;
    state.save().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> Mode {
        Mode::On {
            endpoint: "https://collector".into(),
        }
    }

    #[test]
    fn decide_modes() {
        let ep = || Some("https://collector".to_string());
        assert_eq!(decide(None, None, true, ep()), on());
        for off in ["0", "false", "OFF", "no"] {
            assert_eq!(decide(Some(off), None, true, ep()), Mode::Off, "{off}");
        }
        assert_eq!(decide(None, Some("1"), true, ep()), Mode::Off);
        assert_eq!(decide(None, Some("true"), true, ep()), Mode::Off);
        assert_eq!(decide(None, Some("0"), true, ep()), on());
        assert_eq!(decide(None, None, false, ep()), Mode::Off);
        // No endpoint (source and dev builds) means off.
        assert_eq!(decide(None, None, true, None), Mode::Off);
        assert_eq!(decide(None, None, true, Some(" ".into())), Mode::Off);
        // Inspect mode works on any build and sends nothing.
        assert_eq!(decide(Some("log"), None, true, None), Mode::Log);
        assert_eq!(decide(Some("log"), Some("1"), false, None), Mode::Log);
        // An unrecognised value does not opt out on its own.
        assert_eq!(decide(Some("1"), None, true, ep()), on());
    }

    #[test]
    fn state_file_without_notice_field_deserializes() {
        let state: VersionCheckCache =
            serde_json::from_str(r#"{"last_checked_at":null,"latest_binary":null}"#).unwrap();
        assert!(!state.telemetry_notice_shown);
    }
}
