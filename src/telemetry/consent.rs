//! Whether telemetry runs, and the one-time notice printed before the first
//! event is recorded.

use crate::version_cache::VersionCheckCache;

/// The onboarding service route release builds fetch the send-your-data key
/// from (`key.rs`): always the eu2 host, whatever region the user is on. A
/// constant, not a build-time env var: Homebrew, Cargo and Nix compile the
/// tagged source themselves, so only what is in the source reaches every
/// build. `None` would mean no build sends. Debug builds never use it; they
/// send only when `CX_TELEMETRY_KEY_URL` is set at runtime (tests, staging).
const BUILD_KEY_URL: Option<&str> = Some("https://api.eu2.coralogix.com/api/v2/cli-telemetry-key");

pub const NOTICE: &str = "cx collects usage data (command names, versions, error categories, \
a random install id and your Coralogix team id — never queries, arguments, credentials, or \
personal data) to improve the CLI.\n\
Details: https://github.com/coralogix/cx-cli/blob/master/docs/telemetry.md \
· Opt out: CX_TELEMETRY=0 or `telemetry = false` in ~/.cx/config.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// No event is built.
    Off,
    /// Each event is printed to stderr; nothing is stored or sent. Debug
    /// builds only.
    Log,
    /// Events are spooled and sent with the key fetched from `key_url`.
    On { key_url: String },
}

/// The mode for this process, from the env vars, `~/.cx/config.toml` and the
/// key route URL.
pub fn mode() -> Mode {
    let env = |var| std::env::var(var).ok();
    let config = crate::config::config_file()
        .ok()
        .map(|path| std::fs::read_to_string(path).map_err(|e| e.kind()));
    decide(
        env("CX_TELEMETRY").as_deref(),
        env("DO_NOT_TRACK").as_deref(),
        config_allows(config),
        env("CX_TELEMETRY_KEY_URL").or(BUILD_KEY_URL
            .filter(|_| !cfg!(debug_assertions))
            .map(str::to_string)),
    )
}

/// The `telemetry` key alone, so an error elsewhere in config.toml (or a key
/// from a newer cx) can't turn an opt-out back on. A config that exists but
/// can't be read or parsed counts as an opt-out: it may hold one.
fn config_allows(config: Option<Result<String, std::io::ErrorKind>>) -> bool {
    let raw = match config {
        None | Some(Err(std::io::ErrorKind::NotFound)) => return true,
        Some(Err(_)) => return false,
        Some(Ok(raw)) => raw,
    };
    match raw.parse::<toml::Table>() {
        Ok(table) => table
            .get("telemetry")
            .is_none_or(|v| v.as_bool() == Some(true)),
        Err(_) => false,
    }
}

/// `CX_TELEMETRY` can only turn telemetry off (or to `log`): an "on" value
/// does not override another opt-out. A value it does not know is an opt-out,
/// so a typo or `disabled` never sends. Then any other opt-out; then telemetry
/// needs a route to get its key from.
fn decide(
    cx_telemetry: Option<&str>,
    do_not_track: Option<&str>,
    config_enabled: bool,
    key_url: Option<String>,
) -> Mode {
    let lower = |v: Option<&str>| v.map(|v| v.trim().to_ascii_lowercase());
    match lower(cx_telemetry).as_deref() {
        // Unset, or set but empty (`CX_TELEMETRY=`).
        None | Some("" | "1" | "true" | "on" | "yes") => {}
        Some("log") if cfg!(debug_assertions) => return Mode::Log,
        // Release builds have no Log mode. `log` still means "send nothing".
        Some(_) => return Mode::Off,
    }
    if matches!(
        lower(do_not_track).as_deref(),
        Some("1" | "true" | "yes" | "on")
    ) || !config_enabled
    {
        return Mode::Off;
    }
    match key_url.filter(|u| !u.trim().is_empty()) {
        Some(key_url) => Mode::On { key_url },
        None => Mode::Off,
    }
}

/// Print the notice once, on stderr. Returns whether recording may go ahead.
/// The flag is saved first and the notice printed only if that worked: where
/// `~/.cx` is read-only nothing can be recorded anyway, and the notice must
/// not repeat on every run.
pub fn ensure_notice_shown() -> bool {
    let mut state = VersionCheckCache::load();
    if state.telemetry_notice_shown {
        return true;
    }
    state.telemetry_notice_shown = true;
    if state.save().is_err() {
        return false;
    }
    eprintln!("{NOTICE}");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> Mode {
        Mode::On {
            key_url: "https://key-route".into(),
        }
    }

    #[test]
    fn decide_modes() {
        let ep = || Some("https://key-route".to_string());
        assert_eq!(decide(None, None, true, ep()), on());
        for off in ["0", "false", "OFF", "no"] {
            assert_eq!(decide(Some(off), None, true, ep()), Mode::Off, "{off}");
        }
        assert_eq!(decide(None, Some("1"), true, ep()), Mode::Off);
        assert_eq!(decide(None, Some("true"), true, ep()), Mode::Off);
        assert_eq!(decide(None, Some("0"), true, ep()), on());
        assert_eq!(decide(None, None, false, ep()), Mode::Off);
        // No key route URL (source and dev builds) means off.
        assert_eq!(decide(None, None, true, None), Mode::Off);
        assert_eq!(decide(None, None, true, Some(" ".into())), Mode::Off);
        // An unrecognised value is an opt-out.
        for off in ["disabled", "none", "n", "2", "of"] {
            assert_eq!(decide(Some(off), None, true, ep()), Mode::Off, "{off}");
        }
        for allowed in ["1", "true", "ON", "yes", "", " "] {
            assert_eq!(decide(Some(allowed), None, true, ep()), on(), "{allowed}");
            // An "on" value does not override another opt-out.
            let off = decide(Some(allowed), Some("1"), true, ep());
            assert_eq!(off, Mode::Off, "{allowed}");
        }
    }

    #[test]
    fn log_mode_exists_only_in_debug_builds() {
        let expected = if cfg!(debug_assertions) {
            Mode::Log
        } else {
            Mode::Off
        };
        // Never `On`, whatever the build: `log` means nothing is sent.
        let ep = || Some("https://key-route".to_string());
        assert_eq!(decide(Some("log"), None, true, ep()), expected);
        assert_eq!(decide(Some("LOG"), None, true, None), expected);
        assert_eq!(decide(Some("log"), Some("1"), false, None), expected);
    }

    #[test]
    fn config_opt_out_survives_other_config_errors() {
        let file = |s: &str| Some(Ok(s.to_string()));
        assert!(config_allows(None));
        assert!(config_allows(Some(Err(std::io::ErrorKind::NotFound))));
        assert!(config_allows(file("read_only = true\n")));
        assert!(config_allows(file("telemetry = true\n")));
        assert!(!config_allows(file("telemetry = false\n")));
        // A bad value for another key breaks the Config parse, not this one.
        assert!(!config_allows(file(
            "telemetry = false\nread_only = \"yes\"\n"
        )));
        // Not a bool: read as an opt-out rather than guessed on.
        assert!(!config_allows(file("telemetry = \"false\"\n")));
        // Unparseable or unreadable: it may hold an opt-out.
        assert!(!config_allows(file("telemetry = = false")));
        assert!(!config_allows(Some(Err(
            std::io::ErrorKind::PermissionDenied
        ))));
    }
}
