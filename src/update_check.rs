//! Background version-check and update-notification system.
//!
//! ## Design (mirrors `gh`'s model)
//!
//! * Once per 24 h, [`fetch_if_stale`] contacts the GitHub releases API (or,
//!   for Homebrew installs, the homebrew-core formula API) to discover the
//!   latest version.  Results are cached in `~/.cx/state.json`
//!   (see [`crate::version_cache`]).
//! * Every command reads that cached result and either:
//!   - Prints update notices to **stderr** (see [`maybe_print_notice`]):
//!     colored on TTY text mode; plain single-line `[cx update]` for toon
//!     mode or non-TTY stderr (binary and skills separated by `|`).
//!   - Returns a `_meta.update` JSON block for the toon output path (see
//!     [`build_meta_block`]).
//! * Independently of the binary check, every command compares the cx version
//!   that last installed the global skills (`skills_installed_for` in the state
//!   file) with the running version.  After an upgrade (`brew upgrade cx`) the
//!   binary is current but the skills are not, so a skills-only notice shows.
//!   Skills that cx did not install (found in the installer's lock file) get a
//!   softer notice, once per cx version.  This check is local only: no network
//!   call.
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
const BREW_FORMULA_URL: &str = "https://formulae.brew.sh/api/formula/cx.json";

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
    // Re-read the fields other commands may have written while we fetched
    // (`cx skills install` records its version), so we don't overwrite them.
    let latest_state = VersionCheckCache::load();
    state.installation_id = latest_state.installation_id.or(state.installation_id);
    state.skills_installed_for = latest_state.skills_installed_for;
    state.skills_unknown_noticed_for = latest_state.skills_unknown_noticed_for;
    let _ = state.save();
}

/// Homebrew installs ask homebrew-core instead of GitHub: a release reaches the
/// formula hours after it is tagged, and until then `brew upgrade cx` has
/// nothing to install.
async fn fetch_latest_binary(client: &reqwest::Client) -> Option<String> {
    match install_method::detect() {
        install_method::InstallMethod::Brew => {
            let resp = get_json(client, BREW_FORMULA_URL).await?;
            brew_stable_version(&resp)
        }
        _ => {
            let url = format!("https://api.github.com/repos/{BINARY_REPO}/releases/latest");
            let resp = get_json(client, &url).await?;
            github_release_version(&resp)
        }
    }
}

async fn get_json(client: &reqwest::Client, url: &str) -> Option<Value> {
    client.get(url).send().await.ok()?.json().await.ok()
}

fn github_release_version(resp: &Value) -> Option<String> {
    resp["tag_name"]
        .as_str()
        .map(|s| s.trim_start_matches('v').to_string())
}

fn brew_stable_version(resp: &Value) -> Option<String> {
    resp["versions"]["stable"].as_str().map(str::to_string)
}

// ── Human notice (stderr) ─────────────────────────────────────────────────────

/// Print an update notice to stderr, if `CX_NO_UPDATE_NOTIFIER` is not set and
/// either a newer binary is in the cache or the installed skills need a
/// refresh (see [`SkillsStatus`]).  A newer binary wins: its notice already
/// names the skills refresh.
/// - Toon mode or non-TTY stderr: plain `[cx update]` (agent/script friendly)
/// - Text mode with TTY stderr: colored human notice
pub fn maybe_print_notice(output: OutputFormat) {
    if env_is_truthy("CX_NO_UPDATE_NOTIFIER") {
        return;
    }

    let state = VersionCheckCache::load();
    let current = env!("CARGO_PKG_VERSION");
    let plain = output == OutputFormat::Toon || !std::io::stderr().is_terminal();
    let skills = install_method::skills_upgrade_command();

    if let Some(latest) = state.latest_binary.as_deref() {
        if is_newer(latest, current) {
            let upgrade = install_method::binary_upgrade_command(&install_method::detect());

            if plain {
                print_plain_notice(latest, current, &upgrade, skills);
                return;
            }

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
            return;
        }
    }

    let status = skills_status(&state, current);
    let message = match status {
        SkillsStatus::Current => return,
        SkillsStatus::Outdated => format!("cx agent skills are older than cx {current}"),
        SkillsStatus::Unknown => "cx can't tell which version your cx agent skills are".to_string(),
    };
    if status == SkillsStatus::Unknown {
        crate::version_cache::record_skills_unknown_noticed(current);
    }
    if plain {
        eprintln!("[cx update] {message} | skills: {skills}");
        return;
    }
    eprintln!();
    eprintln!("{}", format!("{message}.").dimmed());
    eprintln!("{} {}", "skills:".dimmed(), skills.bold());
    eprintln!(
        "{}",
        "Set CX_NO_UPDATE_NOTIFIER=1 to silence this.".dimmed()
    );
}

// ── Toon meta block (stdout) ────────────────────────────────────────────────

/// Build a `_meta` JSON object for injection into toon-mode output.
/// Returns `None` when suppressed or when there is nothing to update.
///
/// `update.binary` is present only when a newer binary exists.  `update.skills`
/// is present whenever `update` is; when the skills themselves need a refresh
/// it also carries `reason` (`"outdated"` or `"unknown_version"`) and
/// `installed_for` (the cx version that installed them, `null` if unknown).
pub fn build_meta_block() -> Option<Value> {
    if env_is_truthy("CX_NO_UPDATE_NOTIFIER") {
        return None;
    }
    let state = VersionCheckCache::load();
    let current = env!("CARGO_PKG_VERSION");
    let status = skills_status(&state, current);
    let meta = meta_block_for(&state, current, &status);
    if meta.is_some() && status == SkillsStatus::Unknown {
        crate::version_cache::record_skills_unknown_noticed(current);
    }
    meta
}

fn meta_block_for(
    state: &VersionCheckCache,
    current: &str,
    status: &SkillsStatus,
) -> Option<Value> {
    let binary_block = state.latest_binary.as_deref().and_then(|latest| {
        if is_newer(latest, current) {
            let command = install_method::binary_upgrade_command(&install_method::detect());
            Some(json!({ "current": current, "latest": latest, "command": command }))
        } else {
            None
        }
    });

    let mut skills = json!({
        "command": install_method::skills_upgrade_command(),
        "docs": install_method::install_docs_url()
    });
    let reason = match status {
        SkillsStatus::Current => None,
        SkillsStatus::Outdated => Some("outdated"),
        SkillsStatus::Unknown => Some("unknown_version"),
    };
    if let Some(reason) = reason {
        skills["reason"] = json!(reason);
        skills["installed_for"] = json!(state.skills_installed_for);
    }

    match binary_block {
        Some(b) => Some(json!({ "update": { "binary": b, "skills": skills } })),
        None if reason.is_some() => Some(json!({ "update": { "skills": skills } })),
        None => None,
    }
}

/// Print the `_meta` block to stdout in toon mode.
/// Printed as a standalone JSON object on its own line, after the main output.
pub fn maybe_print_toon_meta() {
    if let Some(meta) = build_meta_block() {
        if let Ok(s) = serde_json::to_string(&json!({ "_meta": meta })) {
            println!("{s}");
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Single-line notice for toon mode and non-TTY stderr.
/// Binary upgrade and skills refresh are separated by `|` so both stay together.
fn print_plain_notice(latest: &str, current: &str, upgrade: &str, skills: &str) {
    eprintln!(
        "[cx update] v{latest} available (you have {current}) | upgrade: {upgrade} | skills: {skills}"
    );
}

#[derive(Debug, PartialEq, Eq)]
enum SkillsStatus {
    /// Installed for this version or newer, no cx skills found at all, or the
    /// unknown-version notice was already shown for this cx version.
    Current,
    /// cx installed the global skills for an older version.
    Outdated,
    /// Global cx skills exist but cx did not record a version for them: they
    /// were installed before cx recorded one, or with `npx skills add`
    /// directly.  They may be current, so this shows once per cx version.
    Unknown,
}

fn skills_status(state: &VersionCheckCache, current: &str) -> SkillsStatus {
    skills_status_with(
        state.skills_installed_for.as_deref(),
        state.skills_unknown_noticed_for.as_deref(),
        current,
        crate::commands::skills::global_lock_has_cx_skills,
    )
}

/// `has_unrecorded_install` runs only when it can change the result, so the
/// lock-file read is skipped for every user who installed through cx.
fn skills_status_with(
    installed_for: Option<&str>,
    unknown_noticed_for: Option<&str>,
    current: &str,
    has_unrecorded_install: impl FnOnce() -> bool,
) -> SkillsStatus {
    match installed_for {
        Some(v) if is_newer(current, v) => SkillsStatus::Outdated,
        Some(_) => SkillsStatus::Current,
        None if unknown_noticed_for == Some(current) => SkillsStatus::Current,
        None if has_unrecorded_install() => SkillsStatus::Unknown,
        None => SkillsStatus::Current,
    }
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
    fn github_release_version_strips_v_prefix() {
        let resp = json!({ "tag_name": "v0.1.29" });
        assert_eq!(github_release_version(&resp).as_deref(), Some("0.1.29"));
        assert_eq!(github_release_version(&json!({})), None);
    }

    #[test]
    fn brew_stable_version_reads_formula_api() {
        let resp = json!({
            "name": "cx",
            "versions": { "stable": "0.1.28", "head": null, "bottle": true }
        });
        assert_eq!(brew_stable_version(&resp).as_deref(), Some("0.1.28"));
        assert_eq!(brew_stable_version(&json!({})), None);
    }

    #[test]
    fn skills_older_than_binary_are_outdated() {
        assert_eq!(
            skills_status_with(Some("0.1.20"), None, "0.1.30", || panic!("no lock read")),
            SkillsStatus::Outdated
        );
    }

    #[test]
    fn skills_for_this_or_newer_version_are_current() {
        assert_eq!(
            skills_status_with(Some("0.1.30"), None, "0.1.30", || panic!("no lock read")),
            SkillsStatus::Current
        );
        // Downgraded binary: never ask to "update" skills to an older version.
        assert_eq!(
            skills_status_with(Some("0.2.0"), None, "0.1.30", || panic!("no lock read")),
            SkillsStatus::Current
        );
    }

    #[test]
    fn unrecorded_skills_depend_on_lock_file() {
        assert_eq!(
            skills_status_with(None, None, "0.1.30", || true),
            SkillsStatus::Unknown
        );
        assert_eq!(
            skills_status_with(None, None, "0.1.30", || false),
            SkillsStatus::Current
        );
    }

    #[test]
    fn unknown_skills_notice_shows_once_per_version() {
        // Already shown for this version: silent, and no lock read.
        assert_eq!(
            skills_status_with(None, Some("0.1.30"), "0.1.30", || panic!("no lock read")),
            SkillsStatus::Current
        );
        // Shown for an older version: show again after the upgrade.
        assert_eq!(
            skills_status_with(None, Some("0.1.29"), "0.1.30", || true),
            SkillsStatus::Unknown
        );
    }

    #[test]
    fn meta_block_unknown_skills_has_reason() {
        let state = VersionCheckCache::default();
        let meta = meta_block_for(&state, "0.1.30", &SkillsStatus::Unknown).expect("unknown");
        assert!(meta["update"].get("binary").is_none());
        assert_eq!(meta["update"]["skills"]["reason"], "unknown_version");
        assert!(meta["update"]["skills"]["installed_for"].is_null());
    }

    #[test]
    fn meta_block_skills_only_after_upgrade() {
        let state = VersionCheckCache {
            latest_binary: Some("0.1.30".into()),
            skills_installed_for: Some("0.1.20".into()),
            ..Default::default()
        };
        let meta =
            meta_block_for(&state, "0.1.30", &SkillsStatus::Outdated).expect("skills are outdated");
        assert!(meta["update"].get("binary").is_none());
        assert_eq!(meta["update"]["skills"]["reason"], "outdated");
        assert_eq!(meta["update"]["skills"]["installed_for"], "0.1.20");
        assert_eq!(
            meta["update"]["skills"]["command"],
            "cx skills install --global"
        );
    }

    #[test]
    fn meta_block_none_when_everything_current() {
        let state = VersionCheckCache {
            latest_binary: Some("0.1.30".into()),
            skills_installed_for: Some("0.1.30".into()),
            ..Default::default()
        };
        assert!(meta_block_for(&state, "0.1.30", &SkillsStatus::Current).is_none());
    }

    #[test]
    fn meta_block_binary_update_keeps_skills_hint() {
        let state = VersionCheckCache {
            latest_binary: Some("0.2.0".into()),
            skills_installed_for: Some("0.1.30".into()),
            ..Default::default()
        };
        let meta =
            meta_block_for(&state, "0.1.30", &SkillsStatus::Current).expect("binary is outdated");
        assert_eq!(meta["update"]["binary"]["latest"], "0.2.0");
        assert!(meta["update"]["skills"].get("reason").is_none());
    }

    #[test]
    fn build_meta_block_suppressed_by_env_var() {
        std::env::set_var("CX_NO_UPDATE_NOTIFIER", "1");
        let result = build_meta_block();
        std::env::remove_var("CX_NO_UPDATE_NOTIFIER");
        assert!(result.is_none());
    }
}
