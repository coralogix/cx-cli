//! Background version-check and update-notification system.
//!
//! ## Design (mirrors `gh`'s model)
//!
//! * Once per 24 h, [`fetch_if_stale`] contacts the GitHub releases API to
//!   discover the latest version.  Results are cached in `~/.cx/state.json`
//!   (see [`crate::version_cache`]).
//! * Every command reads that cached result and either:
//!   - Prints update notices to **stderr** (see [`maybe_print_notice`]):
//!     colored on TTY text mode; plain single-line `[cx update]` for agents
//!     mode or non-TTY stderr (binary and skills separated by `|`).
//!   - Returns a `_meta.update` JSON block for the agents output path (see
//!     [`build_meta_block`]).
//! * `CX_NO_UPDATE_NOTIFIER=1` suppresses all notifications (including the fetch).
//! * `fetch_if_stale` is spawned via `tokio::spawn` and intentionally races
//!   with the command.  For fast local commands (e.g. `cx profiles list`) the
//!   task may be cancelled before it finishes writing; the state is populated
//!   on the next API command.  This accepted race matches `gh`'s behaviour.

use std::io::IsTerminal;

use chrono::Utc;
use colored::Colorize;
use serde_json::{json, Value};

use crate::config::OutputFormat;
use crate::install_method;
use crate::safety::env_is_truthy;
use crate::version_cache::VersionCheckCache;

const BINARY_REPO: &str = "coralogix/cx-cli";

// ── Background fetcher ────────────────────────────────────────────────────────

/// Fetch the latest binary version if the cached data is older than
/// 24 h, then persist the result.  Designed to be run via `tokio::spawn`.
/// All errors are silently swallowed — this must never affect the CLI output.
pub async fn fetch_if_stale() {
    if env_is_truthy("CX_NO_UPDATE_NOTIFIER") {
        return;
    }

    let mut state = VersionCheckCache::load();
    if !state.check_is_stale() {
        return;
    }

    let Ok(client) = reqwest::Client::builder()
        .user_agent(concat!("cx-cli/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
    else {
        return;
    };

    if let Some(v) = fetch_latest_binary(&client).await {
        state.latest_binary = Some(v);
    }

    // Always update the timestamp, even when fetches fail (e.g. GitHub is
    // unreachable).  This prevents hammering the API on every command during
    // an outage — we accept missing one 24 h window rather than spamming.
    state.last_checked_at = Some(Utc::now());
    let _ = state.save();
}

async fn fetch_latest_binary(client: &reqwest::Client) -> Option<String> {
    let url = format!("https://api.github.com/repos/{BINARY_REPO}/releases/latest");
    let resp: Value = client.get(&url).send().await.ok()?.json().await.ok()?;
    resp["tag_name"]
        .as_str()
        .map(|s| s.trim_start_matches('v').to_string())
}

// ── Human notice (stderr) ─────────────────────────────────────────────────────

/// Print a one-line update notice to stderr, if all conditions are met:
/// - `CX_NO_UPDATE_NOTIFIER` is not set
/// - A newer version is available in the cache
/// - Agents mode or non-TTY stderr: plain `[cx update]` (agent/script friendly)
/// - Text mode with TTY stderr: colored human notice
pub fn maybe_print_notice(output: OutputFormat) {
    if env_is_truthy("CX_NO_UPDATE_NOTIFIER") {
        return;
    }

    let state = VersionCheckCache::load();
    let current = env!("CARGO_PKG_VERSION");

    if let Some(latest) = state.latest_binary.as_deref() {
        if is_newer(latest, current) {
            let upgrade = install_method::binary_upgrade_command(&install_method::detect());

            if output == OutputFormat::Agents || !std::io::stderr().is_terminal() {
                print_plain_notice(latest, current, &upgrade);
                return;
            }

            let skills = install_method::skills_upgrade_command();
            eprintln!();
            eprintln!(
                "{} {} {} {}{}.",
                "cx".bold(),
                latest.bold().green(),
                "is available (you have".dimmed(),
                current.dimmed(),
                ")".dimmed()
            );
            eprintln!(
                "{} {} {} {} {}",
                "upgrade:".dimmed(),
                upgrade.bold(),
                "|".dimmed(),
                "skills:".dimmed(),
                skills.bold()
            );
            eprintln!(
                "{}",
                "Set CX_NO_UPDATE_NOTIFIER=1 to silence this.".dimmed()
            );
        }
    }
}

// ── Agents meta block (stdout) ────────────────────────────────────────────────

/// Build a `_meta` JSON object for injection into agents-mode output.
/// Returns `None` when suppressed or when no update is available.
pub fn build_meta_block() -> Option<Value> {
    if env_is_truthy("CX_NO_UPDATE_NOTIFIER") {
        return None;
    }

    let state = VersionCheckCache::load();
    let current = env!("CARGO_PKG_VERSION");

    let binary_block = state.latest_binary.as_deref().and_then(|latest| {
        if is_newer(latest, current) {
            let command = install_method::binary_upgrade_command(&install_method::detect());
            Some(json!({ "current": current, "latest": latest, "command": command }))
        } else {
            None
        }
    });

    binary_block.map(|b| {
        json!({
            "update": {
                "binary": b,
                "skills": {
                    "command": install_method::skills_upgrade_command(),
                    "docs": install_method::install_docs_url()
                }
            }
        })
    })
}

/// Print the `_meta` block to stdout for agents to consume.
/// Printed as a standalone JSON object on its own line, after the main output.
pub fn maybe_print_agents_meta() {
    if let Some(meta) = build_meta_block() {
        if let Ok(s) = serde_json::to_string(&json!({ "_meta": meta })) {
            println!("{s}");
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Single-line notice for agents and non-TTY stderr.
/// Binary upgrade and skills refresh are separated by `|` so both stay together.
fn print_plain_notice(latest: &str, current: &str, upgrade: &str) {
    let skills = install_method::skills_upgrade_command();
    eprintln!(
        "[cx update] v{latest} available (you have {current}) | upgrade: {upgrade} | skills: {skills}"
    );
}

/// Returns true when `latest` is strictly greater than `current`.
///
/// Parses both strings as `(major, minor, patch)` tuples and compares
/// lexicographically.  Returns `false` if either string is unparseable
/// (e.g. a dev build suffix), so we never falsely tell a user to downgrade.
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Parse a `"X.Y.Z"` or `"X.Y.Z-suffix"` version string into a comparable tuple.
///
/// Pre-release suffixes on the patch component are stripped before parsing
/// (e.g. `"0.1.15-dev"` → `(0, 1, 15)`).  This means a dev build on `0.1.15-dev`
/// is treated as `0.1.15` for comparison purposes — so users on pre-release builds
/// still get notified when a strictly newer release ships, and are never falsely
/// told to downgrade to an older published release.
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let v = v.strip_prefix('v').unwrap_or(v);
    let mut parts = v.splitn(3, '.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    // Strip pre-release suffix: "15-dev" → "15", "5" → "5"
    let patch_str = parts.next()?;
    let patch_digits: &str = patch_str
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("");
    let patch: u32 = patch_digits.parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_latest_greater_than_current() {
        assert!(is_newer("1.3.0", "1.2.0"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(is_newer("0.2.0", "0.1.5"));
    }

    #[test]
    fn is_newer_same_version() {
        assert!(!is_newer("1.2.0", "1.2.0"));
    }

    #[test]
    fn is_newer_latest_older_than_current_no_downgrade() {
        // User has a newer build than the published release — must stay silent.
        assert!(!is_newer("0.1.4", "0.1.5"));
        assert!(!is_newer("1.2.0", "1.3.0"));
    }

    #[test]
    fn is_newer_unparseable_returns_false() {
        assert!(!is_newer("", "1.2.0"));
        assert!(!is_newer("1.3.0", ""));
        assert!(!is_newer("not-a-version", "1.2.0"));
    }

    #[test]
    fn is_newer_pre_release_suffix_handled() {
        // Dev build on 0.1.15-dev: newer release 0.2.0 should show notice.
        assert!(is_newer("0.2.0", "0.1.15-dev"));
        // Dev build on 0.1.15-dev: same base version published → no notice.
        assert!(!is_newer("0.1.15", "0.1.15-dev"));
        // Dev build on 0.1.15-dev: older published version → no downgrade notice.
        assert!(!is_newer("0.1.14", "0.1.15-dev"));
        // GitHub never returns pre-release tags, but be safe: treat them as base version.
        assert!(is_newer("0.2.0-rc1", "0.1.5"));
    }

    #[test]
    fn is_newer_v_prefix_handled() {
        // Defensive: strip v prefix if it somehow slips through.
        assert!(is_newer("v1.3.0", "1.2.0"));
        assert!(is_newer("1.3.0", "v1.2.0"));
        assert!(is_newer("v1.3.0", "v1.2.0"));
    }

    #[test]
    fn build_meta_block_suppressed_by_env_var() {
        std::env::set_var("CX_NO_UPDATE_NOTIFIER", "1");
        let result = build_meta_block();
        std::env::remove_var("CX_NO_UPDATE_NOTIFIER");
        assert!(result.is_none());
    }
}
