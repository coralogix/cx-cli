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
//!   softer notice, once per cx version.  Either notice is skipped when the
//!   lock file shows every cx skill updated after this binary was built (for
//!   example by `npx skills add -g`).  This check is local only: no network
//!   call.
//! * In both cases, cx first tries to fix the skills itself: it starts the
//!   hidden `cx skills auto-refresh` in the background (see
//!   [`skills_status_with_refresh`]).  The notices are the fallback when that
//!   is off, impossible (no `npx`), or failed.
//! * `CX_NO_UPDATE_NOTIFIER=1` suppresses all notifications (including the fetch).
//! * `fetch_if_stale` is spawned via `tokio::spawn` and intentionally races
//!   with the command.  For fast local commands (e.g. `cx profiles list`) the
//!   task may be cancelled before it finishes writing; the state is populated
//!   on the next API command.  This accepted race matches `gh`'s behaviour.

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Utc};
use colored::Colorize;
use serde_json::{json, Value};

use crate::commands::skills::LockedCxSkills;
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
    state.skills_agents = latest_state.skills_agents;
    state.skills_refresh_attempted_for = latest_state.skills_refresh_attempted_for;
    state.skills_refresh_attempted_at = latest_state.skills_refresh_attempted_at;
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
    // Before the binary check, so the skills refresh starts even when a newer
    // binary is also out.
    let status = skills_status_with_refresh(&state, current);

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

    let message = match status {
        SkillsStatus::Current => return,
        SkillsStatus::Refreshing => {
            let message = format!("refreshing cx agent skills for cx {current} in the background");
            if plain {
                eprintln!("[cx update] {message}");
            } else {
                eprintln!();
                eprintln!("{}", format!("Your cx agent skills are older than cx {current}. Refreshing them in the background.").dimmed());
                eprintln!(
                    "{}",
                    "Set auto_update_skills = false in ~/.cx/config.toml to turn this off."
                        .dimmed()
                );
            }
            return;
        }
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
/// it also carries `reason` (`"outdated"`, `"unknown_version"`, or
/// `"refreshing"` when cx started the refresh itself) and `installed_for` (the
/// cx version that installed them, `null` if unknown).
pub fn build_meta_block() -> Option<Value> {
    if env_is_truthy("CX_NO_UPDATE_NOTIFIER") {
        return None;
    }
    let state = VersionCheckCache::load();
    let current = env!("CARGO_PKG_VERSION");
    let status = skills_status_with_refresh(&state, current);
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
        SkillsStatus::Refreshing => Some("refreshing"),
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
    /// Installed for this version or newer, refreshed after this binary was
    /// built, or no cx skills found at all.  For display only: also an
    /// unknown version whose notice was already shown for this cx version
    /// (see [`without_repeat_unknown_notice`]).
    Current,
    /// cx installed the global skills for an older version, and they were not
    /// refreshed since.
    Outdated,
    /// Global cx skills exist but cx did not record a version for them: they
    /// were installed before cx recorded one, or with `npx skills add`
    /// directly.  They may be current, so this shows once per cx version.
    Unknown,
    /// Outdated or unknown, and cx just started `cx skills auto-refresh` in
    /// the background to update them.  No action needed.
    Refreshing,
}

/// Set by commands that install skills themselves (`cx skills`, `cx init`),
/// so their own post-command check never starts a second install.
static SKILLS_REFRESH_SUPPRESSED: AtomicBool = AtomicBool::new(false);

pub fn suppress_skills_refresh() {
    SKILLS_REFRESH_SUPPRESSED.store(true, Ordering::Relaxed);
}

/// What to do about outdated or unknown global skills, from [`refresh_decision`].
#[derive(Debug, PartialEq, Eq)]
enum RefreshDecision {
    /// Start the background refresh.
    Start,
    /// A refresh for this version started a few minutes ago and may still be
    /// running: say nothing.
    InProgress,
    /// Do not start one: show the status as it is.
    No,
}

/// How long a started refresh counts as still running.
const REFRESH_RUNNING_FOR: chrono::Duration = chrono::Duration::minutes(15);
/// How long to wait before retrying a refresh that did not update the skills.
const REFRESH_RETRY_AFTER: chrono::Duration = chrono::Duration::hours(24);

/// Pure so every branch is unit-testable.  `allowed` covers the opt-outs and
/// whether `npx` exists; it is called only for outdated or unknown skills.
///
/// Unknown skills (installed with an older cx, `--interactive`, or `npx`
/// directly) are refreshed too: the refresh runs `cx skills install --global`
/// with auto-detected agents, the same command the unknown-version notice
/// asks the user to run, and the same agent detection the installer's own
/// `skills update` uses.
fn refresh_decision(
    status: &SkillsStatus,
    allowed: impl FnOnce() -> bool,
    attempted_for: Option<&str>,
    attempted_at: Option<DateTime<Utc>>,
    current: &str,
    now: DateTime<Utc>,
) -> RefreshDecision {
    let stale = matches!(status, SkillsStatus::Outdated | SkillsStatus::Unknown);
    if !stale || SKILLS_REFRESH_SUPPRESSED.load(Ordering::Relaxed) {
        return RefreshDecision::No;
    }
    if attempted_for == Some(current) {
        if let Some(at) = attempted_at {
            if now - at < REFRESH_RUNNING_FOR {
                return RefreshDecision::InProgress;
            }
            if now - at < REFRESH_RETRY_AFTER {
                return RefreshDecision::No;
            }
        }
    }
    if allowed() {
        RefreshDecision::Start
    } else {
        RefreshDecision::No
    }
}

/// [`skills_status`], plus the automatic refresh: when cx recorded a global
/// install for an older version, or found cx skills it cannot date, start
/// `cx skills auto-refresh` in the background instead of asking the user to
/// run the install.
fn skills_status_with_refresh(state: &VersionCheckCache, current: &str) -> SkillsStatus {
    let status = skills_status(state, current);
    let decision = refresh_decision(
        &status,
        auto_refresh_allowed,
        state.skills_refresh_attempted_for.as_deref(),
        state.skills_refresh_attempted_at,
        current,
        Utc::now(),
    );
    match decision {
        RefreshDecision::Start if spawn_skills_refresh() => {
            crate::version_cache::record_skills_refresh_attempt(current);
            SkillsStatus::Refreshing
        }
        RefreshDecision::InProgress => SkillsStatus::Current,
        _ => without_repeat_unknown_notice(
            status,
            state.skills_unknown_noticed_for.as_deref(),
            current,
        ),
    }
}

/// The unknown-version notice shows once per cx version.  This applies only
/// to what is shown, after the refresh decision: the refresh must still see
/// `Unknown`, so a failed refresh is retried after [`REFRESH_RETRY_AFTER`].
fn without_repeat_unknown_notice(
    status: SkillsStatus,
    unknown_noticed_for: Option<&str>,
    current: &str,
) -> SkillsStatus {
    if status == SkillsStatus::Unknown && unknown_noticed_for == Some(current) {
        SkillsStatus::Current
    } else {
        status
    }
}

/// Off with `CX_NO_SKILLS_AUTO_UPDATE=1` or `auto_update_skills = false` in
/// `config.toml`; impossible without `npx` on `PATH`.
fn auto_refresh_allowed() -> bool {
    !env_is_truthy("CX_NO_SKILLS_AUTO_UPDATE")
        && crate::config::load_config()
            .map(|c| c.auto_update_skills)
            .unwrap_or(true)
        && crate::commands::skills::npx_on_path()
}

/// Start `cx skills auto-refresh` as a detached process with no terminal
/// output, so the user's command neither waits for it nor shows its output.
fn spawn_skills_refresh() -> bool {
    use std::process::{Command, Stdio};
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut cmd = Command::new(exe);
    cmd.args(["skills", "auto-refresh"])
        .env("CX_NO_UPDATE_NOTIFIER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Its own process group (Unix) or a detached console (Windows), so it
    // keeps running after the user's command and terminal session end.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().is_ok()
}

fn skills_status(state: &VersionCheckCache, current: &str) -> SkillsStatus {
    skills_status_with(
        state.skills_installed_for.as_deref(),
        current,
        binary_built_at(),
        crate::commands::skills::global_lock_cx_skills,
    )
}

/// When this binary was built (set by `build.rs`).
fn binary_built_at() -> DateTime<Utc> {
    env!("CX_BUILD_TIME")
        .parse::<i64>()
        .ok()
        .and_then(|secs| DateTime::from_timestamp(secs, 0))
        .unwrap_or_default()
}

/// `read_lock` runs only when it can change the result, so the lock-file read
/// is skipped for every user whose recorded install is current.
///
/// Skills come from the repo's default branch, so skills that the lock file
/// shows as all updated after `built_at` already match this binary, even when
/// they were refreshed with `npx skills add` and cx did not record it.
fn skills_status_with(
    installed_for: Option<&str>,
    current: &str,
    built_at: DateTime<Utc>,
    read_lock: impl FnOnce() -> Option<LockedCxSkills>,
) -> SkillsStatus {
    let refreshed_since_build =
        |lock: &LockedCxSkills| lock.oldest_update.is_some_and(|t| t >= built_at);
    match installed_for {
        Some(v) if is_newer(current, v) => match read_lock() {
            Some(lock) if refreshed_since_build(&lock) => SkillsStatus::Current,
            _ => SkillsStatus::Outdated,
        },
        Some(_) => SkillsStatus::Current,
        None => match read_lock() {
            Some(lock) if refreshed_since_build(&lock) => SkillsStatus::Current,
            Some(_) => SkillsStatus::Unknown,
            None => SkillsStatus::Current,
        },
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

    /// Build time used by the status tests.
    fn built() -> DateTime<Utc> {
        "2026-10-01T00:00:00Z".parse().unwrap()
    }

    /// A lock file whose oldest cx skill was updated at `t`.
    fn lock_at(t: &str) -> Option<LockedCxSkills> {
        Some(LockedCxSkills {
            oldest_update: Some(t.parse().unwrap()),
        })
    }

    #[test]
    fn skills_older_than_binary_are_outdated() {
        assert_eq!(
            skills_status_with(Some("0.1.20"), "0.1.30", built(), || None),
            SkillsStatus::Outdated
        );
        // Refreshed, but before this binary was built: still outdated.
        assert_eq!(
            skills_status_with(Some("0.1.20"), "0.1.30", built(), || {
                lock_at("2026-09-30T23:59:59Z")
            }),
            SkillsStatus::Outdated
        );
        // A cx skill without a readable time: age unknown, so still outdated.
        assert_eq!(
            skills_status_with(Some("0.1.20"), "0.1.30", built(), || {
                Some(LockedCxSkills {
                    oldest_update: None,
                })
            }),
            SkillsStatus::Outdated
        );
    }

    #[test]
    fn outdated_record_but_refreshed_after_build_is_current() {
        // `npx skills add -g` after this release: cx did not record it, but
        // the lock file shows every cx skill updated after the build.
        assert_eq!(
            skills_status_with(Some("0.1.20"), "0.1.30", built(), || {
                lock_at("2026-10-02T08:00:00Z")
            }),
            SkillsStatus::Current
        );
    }

    #[test]
    fn skills_for_this_or_newer_version_are_current() {
        assert_eq!(
            skills_status_with(Some("0.1.30"), "0.1.30", built(), || {
                panic!("no lock read")
            }),
            SkillsStatus::Current
        );
        // Downgraded binary: never ask to "update" skills to an older version.
        assert_eq!(
            skills_status_with(Some("0.2.0"), "0.1.30", built(), || {
                panic!("no lock read")
            }),
            SkillsStatus::Current
        );
    }

    #[test]
    fn unrecorded_skills_depend_on_lock_file() {
        assert_eq!(
            skills_status_with(None, "0.1.30", built(), || {
                lock_at("2026-09-01T00:00:00Z")
            }),
            SkillsStatus::Unknown
        );
        assert_eq!(
            skills_status_with(None, "0.1.30", built(), || None),
            SkillsStatus::Current
        );
        // Installed with `npx skills add` after this release: no notice.
        assert_eq!(
            skills_status_with(None, "0.1.30", built(), || {
                lock_at("2026-10-02T08:00:00Z")
            }),
            SkillsStatus::Current
        );
    }

    #[test]
    fn unknown_skills_notice_shows_once_per_version() {
        // Already shown for this version: hidden.
        assert_eq!(
            without_repeat_unknown_notice(SkillsStatus::Unknown, Some("0.1.30"), "0.1.30"),
            SkillsStatus::Current
        );
        // Shown for an older version: show again after the upgrade.
        assert_eq!(
            without_repeat_unknown_notice(SkillsStatus::Unknown, Some("0.1.29"), "0.1.30"),
            SkillsStatus::Unknown
        );
        // Only the unknown notice is ever hidden.
        assert_eq!(
            without_repeat_unknown_notice(SkillsStatus::Outdated, Some("0.1.30"), "0.1.30"),
            SkillsStatus::Outdated
        );
    }

    #[test]
    fn unknown_skills_stay_eligible_for_refresh_after_the_notice() {
        // The notice was shown for this version after a failed refresh. The
        // status the refresh sees is still Unknown, so the retry can start.
        let status =
            skills_status_with(None, "0.1.30", built(), || lock_at("2026-09-01T00:00:00Z"));
        assert_eq!(status, SkillsStatus::Unknown);
        let at = now() - chrono::Duration::hours(25);
        assert_eq!(
            refresh_decision(&status, || true, Some("0.1.30"), Some(at), "0.1.30", now()),
            RefreshDecision::Start
        );
    }

    #[test]
    fn build_time_is_embedded() {
        assert!(binary_built_at() > "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap());
    }

    fn now() -> DateTime<Utc> {
        "2026-11-10T12:00:00Z".parse().unwrap()
    }

    #[test]
    fn refresh_starts_for_stale_skills_when_allowed() {
        let decide = |status: SkillsStatus, allowed: bool| {
            refresh_decision(&status, || allowed, None, None, "0.1.31", now())
        };
        assert_eq!(decide(SkillsStatus::Outdated, true), RefreshDecision::Start);
        // Opted out, or no npx.
        assert_eq!(decide(SkillsStatus::Outdated, false), RefreshDecision::No);
        // Skills cx cannot date are refreshed too; current ones never.
        assert_eq!(decide(SkillsStatus::Unknown, true), RefreshDecision::Start);
        assert_eq!(decide(SkillsStatus::Unknown, false), RefreshDecision::No);
        assert_eq!(decide(SkillsStatus::Current, true), RefreshDecision::No);
    }

    #[test]
    fn refresh_runs_once_per_version_per_day() {
        let decide = |attempted_for: &str, minutes_ago: i64| {
            refresh_decision(
                &SkillsStatus::Outdated,
                || true,
                Some(attempted_for),
                Some(now() - chrono::Duration::minutes(minutes_ago)),
                "0.1.31",
                now(),
            )
        };
        // Just started: may still be running, say nothing.
        assert_eq!(decide("0.1.31", 5), RefreshDecision::InProgress);
        // Started hours ago and the skills are still old: it failed; no retry yet.
        assert_eq!(decide("0.1.31", 3 * 60), RefreshDecision::No);
        // A day later: retry.
        assert_eq!(decide("0.1.31", 25 * 60), RefreshDecision::Start);
        // The attempt was for an older version: start for this one.
        assert_eq!(decide("0.1.30", 5), RefreshDecision::Start);
    }

    #[test]
    fn meta_block_refreshing_has_reason() {
        let state = VersionCheckCache {
            skills_installed_for: Some("0.1.20".into()),
            ..Default::default()
        };
        let meta = meta_block_for(&state, "0.1.31", &SkillsStatus::Refreshing).expect("refreshing");
        assert!(meta["update"].get("binary").is_none());
        assert_eq!(meta["update"]["skills"]["reason"], "refreshing");
        assert_eq!(meta["update"]["skills"]["installed_for"], "0.1.20");
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
