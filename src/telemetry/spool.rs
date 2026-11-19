//! The local event queue, `~/.cx/telemetry-spool.jsonl`: one JSON event per
//! line, at most [`MAX_LINES`] lines, nothing older than 7 days.
//!
//! The file is untrusted (anyone who can run `cx` can edit it): a line that
//! does not parse as an [`Event`] is dropped. All I/O errors are the caller's
//! to swallow; telemetry never fails a command.
//!
//! To send, the spool is renamed to a unique `.sending` file first, so a
//! concurrent `cx` appending to the spool cannot lose events. The `.sending`
//! file is deleted after the send. One left behind by a killed process is
//! picked up by the next sender once it is older than a minute.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::event::Event;

pub const MAX_LINES: usize = 500;
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const ABANDONED_AFTER: Duration = Duration::from_secs(60);
/// Compact on append once the file is past this size, so a user who only
/// cannot reach the collector has a bounded spool.
const COMPACT_ABOVE_BYTES: u64 = 512 * 1024;
const SENDING_SUFFIX: &str = ".sending";

pub fn path() -> Option<PathBuf> {
    crate::config::config_dir()
        .ok()
        .map(|d| d.join("telemetry-spool.jsonl"))
}

/// Append events, compacting the spool once it grows past
/// [`COMPACT_ABOVE_BYTES`].
pub fn append(spool: &Path, events: &[Event]) -> io::Result<()> {
    if write_lines(spool, events)? > COMPACT_ABOVE_BYTES {
        // Through take/finish, like a flush, so concurrent appends and
        // flushes are neither lost nor duplicated.
        let taken = take(spool, &uuid::Uuid::new_v4().to_string());
        let kept = taken.events.clone();
        taken.finish(spool, &kept);
    }
    Ok(())
}

/// Append events as a single write, so concurrent appenders don't
/// interleave. Returns the spool's size afterwards.
fn write_lines(spool: &Path, events: &[Event]) -> io::Result<u64> {
    if events.is_empty() {
        return Ok(0);
    }
    let mut lines = String::new();
    for event in events {
        lines.push_str(&serde_json::to_string(event)?);
        lines.push('\n');
    }
    if let Some(parent) = spool.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(spool)?;
    file.write_all(lines.as_bytes())?;
    Ok(file.metadata()?.len())
}

/// Valid, unexpired events, oldest first, at most the newest [`MAX_LINES`].
pub fn parse(contents: &str, now_nanos: u64) -> Vec<Event> {
    let oldest = now_nanos.saturating_sub(MAX_AGE.as_nanos() as u64);
    let mut events: Vec<Event> = contents
        .lines()
        .filter_map(|line| serde_json::from_str::<Event>(line).ok())
        .filter(|e| e.time_unix_nano >= oldest)
        .collect();
    events.sort_by_key(|e| e.time_unix_nano);
    let excess = events.len().saturating_sub(MAX_LINES);
    events.drain(..excess);
    events
}

/// Events moved aside for sending. Call [`Taken::finish`] after the send.
pub struct Taken {
    files: Vec<PathBuf>,
    pub events: Vec<Event>,
}

impl Taken {
    /// Put `unsent` back in the spool, then delete the `.sending` files.
    pub fn finish(self, spool: &Path, unsent: &[Event]) {
        // At most MAX_LINES events, so no compaction is needed.
        if write_lines(spool, unsent).is_err() {
            // Keep the `.sending` files: the next flush retries them.
            return;
        }
        for file in self.files {
            let _ = fs::remove_file(file);
        }
    }
}

/// Move the spool aside under a name unique to `token`, plus any `.sending`
/// file a killed process left behind, and read their events.
pub fn take(spool: &Path, token: &str) -> Taken {
    let mut files = Vec::new();
    let mine = with_suffix(spool, &format!(".{token}{SENDING_SUFFIX}"));
    if claim(spool, &mine) {
        files.push(mine);
    }
    // Claim abandoned files by renaming them too: if two processes see the
    // same one, only the rename that wins reads it.
    for (i, file) in abandoned(spool).into_iter().enumerate() {
        let claimed = with_suffix(spool, &format!(".{token}-{i}{SENDING_SUFFIX}"));
        if claim(&file, &claimed) {
            files.push(claimed);
        }
    }

    let contents: String = files
        .iter()
        .filter_map(|f| fs::read_to_string(f).ok())
        .collect::<Vec<_>>()
        .join("\n");
    Taken {
        events: parse(&contents, now_nanos()),
        files,
    }
}

/// Rename `from` to `to`, then restamp it: a rename keeps the modified time
/// of the last write, and the age must be this flush's, or another process
/// could take the in-progress flush for an abandoned one.
fn claim(from: &Path, to: &Path) -> bool {
    if fs::rename(from, to).is_err() {
        return false;
    }
    let _ = fs::File::options()
        .append(true)
        .open(to)
        .and_then(|f| f.set_modified(SystemTime::now()));
    true
}

/// `.sending` files next to `spool` older than [`ABANDONED_AFTER`]. A younger
/// one belongs to a flush still in progress.
fn abandoned(spool: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (spool.parent(), spool.file_name()) else {
        return Vec::new();
    };
    let prefix = format!("{}.", name.to_string_lossy());
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(&prefix) && n.ends_with(SENDING_SUFFIX)
        })
        .filter(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > ABANDONED_AFTER)
        })
        .map(|e| e.path())
        .collect()
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

pub fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::event::EventKind;
    use uuid::Uuid;

    fn event_at(time_unix_nano: u64) -> Event {
        Event {
            time_unix_nano,
            ..Event::now(Uuid::new_v4(), None, EventKind::FirstRun)
        }
    }

    fn temp_spool(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cx_spool_{tag}_{}_{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("telemetry-spool.jsonl")
    }

    #[test]
    fn append_take_finish_round_trip() {
        let spool = temp_spool("roundtrip");
        let events: Vec<Event> = (0..3).map(|_| event_at(now_nanos())).collect();
        append(&spool, &events[..2]).unwrap();
        append(&spool, &events[2..]).unwrap();

        let taken = take(&spool, "t1");
        assert_eq!(taken.events, events);
        assert!(!spool.exists(), "spool is moved aside while sending");

        // Appends during the send go to a fresh spool and are not lost.
        let late = event_at(now_nanos());
        append(&spool, std::slice::from_ref(&late)).unwrap();

        taken.finish(&spool, &events[2..]);
        let left = take(&spool, "t2");
        assert_eq!(left.events.len(), 2, "late event + unsent event remain");
        assert!(left.events.contains(&late) && left.events.contains(&events[2]));
        left.finish(&spool, &[]);
        let dir = spool.parent().unwrap();
        assert_eq!(fs::read_dir(dir).unwrap().count(), 0, "no files left");
    }

    #[test]
    fn corrupt_and_expired_lines_are_dropped() {
        let now = now_nanos();
        let fresh = event_at(now);
        let expired = event_at(now - MAX_AGE.as_nanos() as u64 - 1);
        let contents = [
            serde_json::to_string(&fresh).unwrap(),
            "{not json".to_string(),
            r#"{"event.name":"cli.evil"}"#.to_string(),
            serde_json::to_string(&expired).unwrap(),
        ]
        .join("\n");
        assert_eq!(parse(&contents, now), vec![fresh]);
    }

    #[test]
    fn cap_keeps_newest() {
        let now = now_nanos();
        let lines: Vec<String> = (0..MAX_LINES as u64 + 20)
            .map(|i| serde_json::to_string(&event_at(now - 10_000 + i)).unwrap())
            .collect();
        let events = parse(&lines.join("\n"), now);
        assert_eq!(events.len(), MAX_LINES);
        assert_eq!(events[0].time_unix_nano, now - 10_000 + 20);
    }

    #[test]
    fn missing_spool_takes_nothing() {
        let spool = temp_spool("missing");
        let taken = take(&spool, "t");
        assert!(taken.events.is_empty());
        taken.finish(&spool, &[]);
        assert!(!spool.exists());
    }

    fn age(path: &Path, by: Duration) {
        let file = fs::File::options().append(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - by).unwrap();
    }

    #[test]
    fn spool_last_written_long_ago_is_read_once() {
        let spool = temp_spool("old_spool");
        append(&spool, &[event_at(now_nanos())]).unwrap();
        age(&spool, Duration::from_secs(600));

        let taken = take(&spool, "t");
        assert_eq!(taken.events.len(), 1, "not read again as abandoned");
        // Another process flushing now must not take this in-progress file.
        assert!(take(&spool, "other").events.is_empty());
        taken.finish(&spool, &[]);
    }

    #[test]
    fn abandoned_sending_files_are_picked_up_after_a_minute() {
        let spool = temp_spool("abandoned");
        let fresh = with_suffix(&spool, ".young.sending");
        let stale = with_suffix(&spool, ".dead.sending");
        for file in [&fresh, &stale] {
            append(file, &[event_at(now_nanos())]).unwrap();
        }
        age(&stale, ABANDONED_AFTER + Duration::from_secs(5));

        let taken = take(&spool, "t");
        assert_eq!(taken.events.len(), 1, "only the stale file is taken");
        taken.finish(&spool, &[]);
        assert!(!stale.exists() && fresh.exists());
    }

    #[test]
    fn append_compacts_an_oversized_spool() {
        let spool = temp_spool("compact");
        let now = now_nanos();
        // ~1 MB, well past COMPACT_ABOVE_BYTES whatever the line length (the
        // agent name varies by environment).
        let events: Vec<Event> = (0..6000).map(|i| event_at(now - 6000 + i)).collect();
        let size: usize = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap().len() + 1)
            .sum();
        assert!(
            size as u64 > COMPACT_ABOVE_BYTES * 3 / 2,
            "test spool too small: {size}"
        );
        append(&spool, &events).unwrap();
        let lines = fs::read_to_string(&spool).unwrap().lines().count();
        assert_eq!(lines, MAX_LINES);
    }
}
