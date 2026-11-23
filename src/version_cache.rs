//! Persistent cache file (`~/.cx/state.json`) for version-check results.
//!
//! This file is managed by `cx`; users should not edit it.
//! All errors are silently swallowed — state is best-effort, never fatal.

use chrono::{DateTime, Duration, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, Instant};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

const LOCK_TIMEOUT: StdDuration = StdDuration::from_millis(100);
const LOCK_RETRY_INTERVAL: StdDuration = StdDuration::from_millis(5);

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

    /// The `cx` version that last ran `cx skills install --global` successfully.
    ///
    /// The skills ship from the same repo as the binary, so the CLI version is
    /// the skills version. Only global installs are recorded: a local install
    /// belongs to one project, and this file is shared by all of them. `None`
    /// means cx never installed global skills (or did so before this field
    /// existed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills_installed_for: Option<String>,

    /// The `cx` version that last showed the "unknown skills version" notice.
    /// That notice shows once per cx version, because cx cannot tell whether
    /// skills it did not install are old.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills_unknown_noticed_for: Option<String>,

    /// The agents passed to that global install (`--agent`). Empty means the
    /// installer auto-detected them. The background refresh repeats them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills_agents: Vec<String>,

    /// The `cx` version for which the background skills refresh last started,
    /// and when. Limits it to one attempt per version per day, so a failing
    /// refresh (offline, installer error) does not run after every command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills_refresh_attempted_for: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills_refresh_attempted_at: Option<DateTime<Utc>>,
}

#[derive(Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(flatten)]
    cache: VersionCheckCache,
    #[serde(flatten)]
    additional_fields: serde_json::Map<String, serde_json::Value>,
}

impl StateFile {
    fn write_to(&self, path: &Path) -> anyhow::Result<()> {
        let contents = serde_json::to_vec_pretty(self)?;
        crate::atomic_file::write(path, &contents)?;
        Ok(())
    }
}

fn read_state<T: Default + DeserializeOwned>(path: &Path) -> io::Result<T> {
    match fs::read(path) {
        Ok(contents) => Ok(serde_json::from_slice(&contents).unwrap_or_default()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error),
    }
}

impl VersionCheckCache {
    fn path() -> anyhow::Result<PathBuf> {
        Ok(crate::config::config_dir()?.join("state.json"))
    }

    /// Load state from disk, returning `Default` on any error.
    pub fn load() -> Self {
        let Ok(path) = Self::path() else {
            return Self::default();
        };
        read_state(&path).unwrap_or_default()
    }

    /// Persist state to disk. Errors are non-fatal.
    pub fn save(&self) -> anyhow::Result<()> {
        let Ok(path) = Self::path() else {
            return Ok(());
        };
        let _lock = lock_state(&path)?;
        let mut state: StateFile = read_state(&path)?;
        state.cache = self.clone();
        state.write_to(&path)
    }

    /// Read current state, mutate it and persist it while holding the writer lock.
    pub(crate) fn update(change: impl FnOnce(&mut Self)) -> anyhow::Result<()> {
        Self::with_locked_state(|path, mut state| {
            change(&mut state.cache);
            state.write_to(path)
        })
    }

    fn with_locked_state<T>(
        change: impl FnOnce(&Path, StateFile) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let path = Self::path()?;
        let _lock = lock_state(&path)?;
        change(&path, read_state(&path)?)
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

fn lock_state(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Keep a separate lock inode so replacement of state.json does not change the lock.
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options.open(path.with_extension("lock"))?;

    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "version cache is locked",
                    ));
                }
                std::thread::sleep(remaining.min(LOCK_RETRY_INTERVAL));
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
}

/// Record a successful global skills install for the running `cx` version,
/// with the agents it targeted.
pub fn record_global_skills_install(agents: &[String]) {
    let _ = VersionCheckCache::update(|state| {
        state.skills_installed_for = Some(env!("CARGO_PKG_VERSION").to_string());
        state.skills_agents = agents.to_vec();
    });
}

/// Record that the background skills refresh started for `version` now.
pub fn record_skills_refresh_attempt(version: &str) {
    let _ = VersionCheckCache::update(|state| {
        state.skills_refresh_attempted_for = Some(version.to_string());
        state.skills_refresh_attempted_at = Some(Utc::now());
    });
}

/// Record that the "unknown skills version" notice was shown for `version`.
pub fn record_skills_unknown_noticed(version: &str) {
    let _ = VersionCheckCache::update(|state| {
        state.skills_unknown_noticed_for = Some(version.to_string());
    });
}

/// Returns the stable random identifier for this local CLI installation.
///
/// The value is generated only when missing and persisted best-effort in the
/// existing `~/.cx/state.json` cache file.
pub fn installation_id() -> String {
    VersionCheckCache::load()
        .installation_id
        .or_else(|| {
            VersionCheckCache::with_locked_state(|path, mut state| {
                if let Some(id) = state.cache.installation_id {
                    return Ok(id);
                }
                let id = state.cache.ensure_installation_id();
                state.write_to(path)?;
                Ok(id)
            })
            .ok()
        })
        .or_else(|| VersionCheckCache::load().installation_id)
        .unwrap_or_else(|| Uuid::new_v4().to_string())
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
        assert!(state.skills_unknown_noticed_for.is_none());
    }

    #[test]
    fn skills_fields_round_trip() {
        let state = VersionCheckCache {
            skills_installed_for: Some("0.1.30".into()),
            skills_unknown_noticed_for: Some("0.1.29".into()),
            skills_agents: vec!["cursor".into()],
            skills_refresh_attempted_for: Some("0.1.30".into()),
            skills_refresh_attempted_at: Some(Utc::now()),
            ..Default::default()
        };
        let json = serde_json::to_string(&state).unwrap();
        let reloaded: VersionCheckCache = serde_json::from_str(&json).unwrap();
        assert_eq!(reloaded.skills_installed_for.as_deref(), Some("0.1.30"));
        assert_eq!(
            reloaded.skills_unknown_noticed_for.as_deref(),
            Some("0.1.29")
        );
        assert_eq!(reloaded.skills_agents, vec!["cursor".to_string()]);
        assert_eq!(
            reloaded.skills_refresh_attempted_for.as_deref(),
            Some("0.1.30")
        );
        assert!(reloaded.skills_refresh_attempted_at.is_some());
    }
}
