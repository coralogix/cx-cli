//! Persistent cache file (`~/.cx/state.json`) for version-check results.
//!
//! This file is managed by `cx`; users should not edit it.
//! All errors are silently swallowed — state is best-effort, never fatal.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::commands::skills::SkillsScope;

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

    /// The `cx` version that last ran `cx skills install` successfully.
    ///
    /// The skills ship from the same repo as the binary, so the CLI version is
    /// the skills version. `None` means cx never installed skills (or did so
    /// before this field existed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills_installed_for: Option<String>,

    /// The scope of that install, so the refresh hint can repeat it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills_scope: Option<SkillsScope>,
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
        std::fs::write(path, contents)?;
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

/// Record a successful skills install for the running `cx` version.
/// `scope` is `None` when the user picked it inside the installer's own flow.
pub fn record_skills_install(scope: Option<SkillsScope>) {
    let mut state = VersionCheckCache::load();
    state.skills_installed_for = Some(env!("CARGO_PKG_VERSION").to_string());
    state.skills_scope = scope;
    let _ = state.save();
}

/// Returns the stable random identifier for this local CLI installation.
///
/// The value is generated only when missing and persisted best-effort in the
/// existing `~/.cx/state.json` cache file.
pub fn installation_id() -> String {
    let mut state = VersionCheckCache::load();
    let was_missing = state.installation_id.is_none();
    let installation_id = state.ensure_installation_id();
    if was_missing {
        let _ = state.save();
    }
    installation_id
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
        assert!(state.skills_installed_for.is_none());
        assert!(state.skills_scope.is_none());
    }

    #[test]
    fn skills_fields_round_trip() {
        let state = VersionCheckCache {
            skills_installed_for: Some("0.1.30".into()),
            skills_scope: Some(SkillsScope::Local),
            ..Default::default()
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains(r#""skills_scope":"local""#));
        let reloaded: VersionCheckCache = serde_json::from_str(&json).unwrap();
        assert_eq!(reloaded.skills_installed_for.as_deref(), Some("0.1.30"));
        assert_eq!(reloaded.skills_scope, Some(SkillsScope::Local));
    }
}
