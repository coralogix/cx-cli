//! Helpers for spilling large non-aggregated Dataprime results to disk when
//! the serialized `agents` payload would exceed `max_dataprime_direct_output_size`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde_json::Value;

/// Prefix used on all result files written by `cx`.
pub const FILE_PREFIX: &str = "cx_results";

/// Age threshold after which result files are considered stale and can be
/// removed by the cleanup command.
pub const CLEANUP_AGE: Duration = Duration::from_secs(30 * 60);

/// Metadata fields that carry no signal for an AI agent and are stripped from
/// `agents` output to reduce token usage.
const METADATA_OMIT: &[&str] = &[
    "branchid",
    "priorityclass",
    "processingOutputTimestampMicros",
    "processingOutputTimestampNanos",
    "timestampMicros",
];

/// Outcome of [`maybe_spill`].
pub enum SpillOutcome {
    /// Payload fits within the configured limit; the caller should print it.
    Direct(String),
    /// Payload exceeded the limit and was written to the returned path.
    Spilled { path: PathBuf, count: usize },
}

/// Transform a single normalized Dataprime row for `agents` output:
///
/// - `metadata` → `$m` (with noisy fields removed)
/// - `labels`   → `$l`
/// - `userData` → `$d`
/// - All other top-level keys are kept unchanged.
pub fn transform_for_agents(row: &Value) -> Value {
    let obj = match row {
        Value::Object(m) => m,
        other => return other.clone(),
    };

    let mut out = serde_json::Map::new();

    for (key, val) in obj {
        match key.as_str() {
            "metadata" => {
                let filtered = match val {
                    Value::Object(meta) => Value::Object(
                        meta.iter()
                            .filter(|(k, _)| !METADATA_OMIT.contains(&k.as_str()))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    ),
                    other => other.clone(),
                };
                out.insert("$m".to_string(), filtered);
            }
            "labels" => {
                out.insert("$l".to_string(), val.clone());
            }
            "userData" => {
                out.insert("$d".to_string(), val.clone());
            }
            _ => {
                out.insert(key.clone(), val.clone());
            }
        }
    }

    Value::Object(out)
}

/// Serialize `raw_results` as pretty JSON and either return it for direct
/// printing, or write it to a `cx_results_<hash>.json` file in `temp_dir`
/// when its byte length exceeds `max_bytes`.
///
/// Pass `max_bytes = None` to disable the limit and always return
/// [`SpillOutcome::Direct`].
///
/// The caller is responsible for any pre-serialization transformations (e.g.
/// [`transform_for_agents`]).
pub fn maybe_spill(
    raw_results: &[Value],
    max_bytes: Option<usize>,
    temp_dir: &str,
) -> Result<SpillOutcome> {
    let json = serde_json::to_string_pretty(raw_results)
        .context("Failed to serialize Dataprime results")?;

    let limit_exceeded = match max_bytes {
        Some(limit) => json.len() > limit,
        None => false,
    };

    if !limit_exceeded {
        return Ok(SpillOutcome::Direct(json));
    }

    let path = write_to_temp(&json, temp_dir)?;
    Ok(SpillOutcome::Spilled {
        path,
        count: raw_results.len(),
    })
}

/// Write `content` to a `cx_results_<hash>.json` file inside `temp_dir`.
/// Returns the full path to the written file.
fn write_to_temp(content: &str, temp_dir: &str) -> Result<PathBuf> {
    let hash = short_hash(content);
    let filename = format!("{FILE_PREFIX}_{hash}.json");
    let path = Path::new(temp_dir).join(&filename);
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write results to {}", path.display()))?;
    Ok(path)
}

/// Delete all `cx_results*` files in `temp_dir` that are older than
/// [`CLEANUP_AGE`]. Returns the number of files removed.
pub fn cleanup_old_files(temp_dir: &str) -> Result<usize> {
    let dir = Path::new(temp_dir);
    let threshold = SystemTime::now()
        .checked_sub(CLEANUP_AGE)
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("Cannot read temp directory: {}", dir.display()))?;

    let mut removed = 0usize;
    for entry in entries {
        let entry = entry.context("Failed to read directory entry")?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if !name_str.starts_with(FILE_PREFIX) {
            continue;
        }

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue, // skip entries whose metadata can't be read
        };
        if let Ok(modified) = meta.modified() {
            if modified < threshold {
                let path = entry.path();
                if let Err(e) = std::fs::remove_file(&path) {
                    eprintln!("Warning: could not remove {}: {e}", path.display());
                } else {
                    removed += 1;
                }
            }
        }
    }

    Ok(removed)
}

/// Derive a short 8-character hex hash from string contents using a simple
/// FNV-1a fold. No external dependency needed.
fn short_hash(s: &str) -> String {
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;
    let mut hash: u64 = FNV_OFFSET;
    for byte in s.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}").chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn short_hash_is_eight_chars() {
        let h = short_hash("hello world");
        assert_eq!(h.len(), 8);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn short_hash_is_deterministic() {
        assert_eq!(short_hash("test"), short_hash("test"));
    }

    #[test]
    fn short_hash_differs_on_different_input() {
        assert_ne!(short_hash("aaa"), short_hash("bbb"));
    }

    #[test]
    fn maybe_spill_direct_when_under_limit() {
        let results = vec![json!({"key": "value"})];
        let expected = serde_json::to_string_pretty(&results).unwrap();
        let limit = Some(expected.len() + 100);
        let outcome = maybe_spill(&results, limit, "/tmp/").unwrap();
        match outcome {
            SpillOutcome::Direct(s) => assert_eq!(s, expected),
            SpillOutcome::Spilled { .. } => panic!("Expected direct output"),
        }
    }

    #[test]
    fn maybe_spill_direct_when_disabled() {
        let results: Vec<Value> = (0..50)
            .map(|i| json!({"index": i, "data": "payload"}))
            .collect();
        // None means no limit — always direct regardless of size.
        let outcome = maybe_spill(&results, None, "/tmp/").unwrap();
        match outcome {
            SpillOutcome::Direct(_) => {}
            SpillOutcome::Spilled { .. } => panic!("Expected direct output when limit is None"),
        }
    }

    #[test]
    fn maybe_spill_writes_file_when_over_limit() {
        let results: Vec<Value> = (0..10)
            .map(|i| json!({"index": i, "data": "some payload"}))
            .collect();
        let expected = serde_json::to_string_pretty(&results).unwrap();
        let limit = Some(expected.len() / 2);

        let tmp = std::env::temp_dir();
        let tmp_str = tmp.to_str().unwrap();

        let outcome = maybe_spill(&results, limit, tmp_str).unwrap();
        match outcome {
            SpillOutcome::Direct(_) => panic!("Expected spill"),
            SpillOutcome::Spilled { path, count } => {
                assert_eq!(count, 10);
                let fname = path.file_name().unwrap().to_string_lossy();
                assert!(fname.starts_with(FILE_PREFIX));
                assert!(fname.ends_with(".json"));
                let written = std::fs::read_to_string(&path).unwrap();
                assert_eq!(written, expected);
                let _ = std::fs::remove_file(path);
            }
        }
    }

    // ── transform_for_agents ──────────────────────────────────────────────────

    #[test]
    fn transform_renames_top_level_keys() {
        let row = json!({
            "metadata": {"severity": "3", "timestamp": "2026-01-01T00:00:00Z"},
            "labels":   {"applicationname": "api"},
            "userData": {"message": "hello"}
        });
        let out = transform_for_agents(&row);
        assert!(out.get("$m").is_some(), "$m should be present");
        assert!(out.get("$l").is_some(), "$l should be present");
        assert!(out.get("$d").is_some(), "$d should be present");
        assert!(
            out.get("metadata").is_none(),
            "original 'metadata' key must be gone"
        );
        assert!(
            out.get("labels").is_none(),
            "original 'labels' key must be gone"
        );
        assert!(
            out.get("userData").is_none(),
            "original 'userData' key must be gone"
        );
    }

    #[test]
    fn transform_strips_noisy_metadata_fields() {
        let row = json!({
            "metadata": {
                "severity": "5",
                "branchid": "abc",
                "priorityclass": "high",
                "processingOutputTimestampMicros": 123,
                "processingOutputTimestampNanos": 456,
                "timestampMicros": 789,
                "timestamp": "2026-01-01T00:00:00Z"
            },
            "labels": {},
            "userData": {}
        });
        let out = transform_for_agents(&row);
        let m = out.get("$m").unwrap();
        assert_eq!(m.get("severity").unwrap(), "5");
        assert_eq!(m.get("timestamp").unwrap(), "2026-01-01T00:00:00Z");
        assert!(m.get("branchid").is_none());
        assert!(m.get("priorityclass").is_none());
        assert!(m.get("processingOutputTimestampMicros").is_none());
        assert!(m.get("processingOutputTimestampNanos").is_none());
        assert!(m.get("timestampMicros").is_none());
    }

    #[test]
    fn transform_preserves_non_special_keys() {
        let row = json!({"some_other_field": 42, "another": "value"});
        let out = transform_for_agents(&row);
        assert_eq!(out.get("some_other_field").unwrap(), 42);
        assert_eq!(out.get("another").unwrap(), "value");
    }

    #[test]
    fn transform_non_object_is_returned_as_is() {
        let row = json!("just a string");
        let out = transform_for_agents(&row);
        assert_eq!(out, row);
    }

    #[test]
    fn cleanup_ignores_files_without_prefix() {
        let tmp = std::env::temp_dir();
        let tmp_str = tmp.to_str().unwrap();

        // A file that does NOT start with the prefix should never be removed.
        let unrelated = tmp.join("some_unrelated_file.json");
        std::fs::write(&unrelated, "{}").unwrap();

        // cleanup_old_files must not panic and must not touch unrelated file.
        let _ = cleanup_old_files(tmp_str);
        assert!(unrelated.exists(), "Unrelated file should not be removed");
        let _ = std::fs::remove_file(unrelated);
    }

    #[test]
    fn cleanup_new_files_not_removed() {
        let tmp = std::env::temp_dir();
        let tmp_str = tmp.to_str().unwrap();

        // A freshly written cx_results file is newer than the threshold.
        let path = tmp.join(format!("{FILE_PREFIX}_cleanup_test_new.json"));
        std::fs::write(&path, "{}").unwrap();

        let removed = cleanup_old_files(tmp_str).unwrap();
        // The file we just created should still be there.
        assert!(path.exists(), "Freshly created file must not be removed");
        let _ = std::fs::remove_file(path);
        let _ = removed; // count may include leftover files from other tests
    }
}
