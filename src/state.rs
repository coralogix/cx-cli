//! Persistent state file (`~/.cx/state.json`) for caching version-check results.
//!
//! This file is managed by `cx`; users should not edit it.
//! All errors are silently swallowed — state is best-effort, never fatal.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// When we last fetched version info from GitHub / npm.
    pub last_checked_at: Option<DateTime<Utc>>,

    /// When we last printed a human-readable notice to stderr.
    pub last_notified_at: Option<DateTime<Utc>>,

    /// Latest `cx` binary version tag fetched from the GitHub releases API
    /// (leading `v` stripped, e.g. `"1.3.0"`).
    pub latest_binary: Option<String>,

    /// Latest skills package version fetched from the npm registry.
    pub latest_skills: Option<String>,
}

impl State {
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

    /// True if the human stderr notice should be shown again (older than 24 h or never shown).
    pub fn notify_is_stale(&self) -> bool {
        match self.last_notified_at {
            None => true,
            Some(t) => Utc::now() - t > Duration::hours(24),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_check_is_stale() {
        let s = State::default();
        assert!(s.check_is_stale(), "never-checked state must be stale");
    }

    #[test]
    fn default_state_notify_is_stale() {
        let s = State::default();
        assert!(s.notify_is_stale(), "never-notified state must be stale");
    }

    #[test]
    fn fresh_check_is_not_stale() {
        let s = State {
            last_checked_at: Some(Utc::now()),
            ..Default::default()
        };
        assert!(!s.check_is_stale());
    }

    #[test]
    fn old_check_is_stale() {
        let s = State {
            last_checked_at: Some(Utc::now() - Duration::hours(25)),
            ..Default::default()
        };
        assert!(s.check_is_stale());
    }

    #[test]
    fn fresh_notify_is_not_stale() {
        let s = State {
            last_notified_at: Some(Utc::now()),
            ..Default::default()
        };
        assert!(!s.notify_is_stale());
    }

    #[test]
    fn old_notify_is_stale() {
        let s = State {
            last_notified_at: Some(Utc::now() - Duration::hours(25)),
            ..Default::default()
        };
        assert!(s.notify_is_stale());
    }
}
