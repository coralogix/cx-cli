//! Persistent cache file (`~/.cx/state.json`) for version-check results.
//!
//! This file is managed by `cx`; users should not edit it.
//! All errors are silently swallowed — state is best-effort, never fatal.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VersionCheckCache {
    /// When we last fetched version info from GitHub.
    pub last_checked_at: Option<DateTime<Utc>>,

    /// Latest `cx` binary version tag fetched from the GitHub releases API
    /// (leading `v` stripped, e.g. `"1.3.0"`).
    pub latest_binary: Option<String>,

    /// Random identifier for this local CLI installation.
    ///
    /// Optional so existing state files continue to deserialize. A UUID is
    /// generated and persisted on the next command invocation when absent.
    pub installation_id: Option<String>,

    /// Set once the telemetry notice has been printed (see `telemetry::consent`).
    #[serde(default)]
    pub telemetry_notice_shown: bool,

    /// The `cx` version of the last run, to record `cli.version_changed`.
    #[serde(default)]
    pub last_run_version: Option<String>,

    /// Set once a query command has succeeded, to record the onboarding step
    /// `first_query_succeeded` once per install.
    #[serde(default)]
    pub first_query_succeeded: bool,
}

impl VersionCheckCache {
    fn path() -> Option<PathBuf> {
        crate::config::config_dir()
            .ok()
            .map(|d| d.join("state.json"))
    }

    /// Load state from disk, returning `Default` on any error.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(contents) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&contents).unwrap_or_default()
    }

    /// Persist state to disk. Errors are non-fatal.
    pub fn save(&self) -> anyhow::Result<()> {
        let Some(path) = Self::path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = serde_json::to_string_pretty(self)?;
        // Write a unique temp file, then rename it over the state file, so a
        // concurrent `cx` never reads a half-written file (and falls back to
        // defaults, wiping every field on its next save).
        let tmp = path.with_extension(format!("json.{}.tmp", Uuid::new_v4()));
        std::fs::write(&tmp, contents)?;
        std::fs::rename(&tmp, &path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        Ok(())
    }

    /// True if version info should be re-fetched (older than 24 h or never fetched).
    pub fn check_is_stale(&self) -> bool {
        match self.last_checked_at {
            None => true,
            Some(t) => Utc::now() - t > Duration::hours(24),
        }
    }

    fn ensure_installation_id(&mut self) -> String {
        self.installation_id
            .get_or_insert_with(|| Uuid::new_v4().to_string())
            .clone()
    }
}

/// Returns the stable random identifier for this local CLI installation.
///
/// The value is generated only when missing and persisted best-effort in the
/// existing `~/.cx/state.json` cache file.
pub fn installation_id() -> String {
    installation_id_first_run().0
}

/// Like [`installation_id`], also saying whether this is the install's first
/// run: `state.json` did not exist and was created now. An existing state file
/// without an id (written by versions before telemetry) is an upgrade, not a
/// new install.
pub fn installation_id_first_run() -> (String, bool) {
    let existed = VersionCheckCache::path().is_some_and(|p| p.exists());
    let mut state = VersionCheckCache::load();
    let was_missing = state.installation_id.is_none();
    let installation_id = state.ensure_installation_id();
    let saved = was_missing && state.save().is_ok();
    (installation_id, !existed && saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_check_is_stale() {
        let s = VersionCheckCache::default();
        assert!(s.check_is_stale(), "never-checked state must be stale");
    }

    #[test]
    fn fresh_check_is_not_stale() {
        let s = VersionCheckCache {
            last_checked_at: Some(Utc::now()),
            ..Default::default()
        };
        assert!(!s.check_is_stale());
    }

    #[test]
    fn old_check_is_stale() {
        let s = VersionCheckCache {
            last_checked_at: Some(Utc::now() - Duration::hours(25)),
            ..Default::default()
        };
        assert!(s.check_is_stale());
    }

    #[test]
    fn installation_id_is_stable_once_generated() {
        let mut state = VersionCheckCache::default();
        let first = state.ensure_installation_id();
        let persisted = serde_json::to_string(&state).unwrap();
        let mut reloaded: VersionCheckCache = serde_json::from_str(&persisted).unwrap();
        let second = reloaded.ensure_installation_id();

        assert_eq!(first, second);
        assert!(Uuid::parse_str(&first).is_ok());
    }

    #[test]
    fn existing_state_without_installation_id_deserializes() {
        let state: VersionCheckCache =
            serde_json::from_str(r#"{"last_checked_at":null,"latest_binary":null}"#).unwrap();
        assert!(state.installation_id.is_none());
    }
}
