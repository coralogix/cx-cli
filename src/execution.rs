//! Shared fan-out / fan-in execution layer.
//!
//! This module provides the primitives for running the same command against
//! multiple named profiles concurrently and then combining their results
//! before a single render pass.  The same pattern extends naturally to
//! multi-region fan-out in the future by adding additional target dimensions.

use std::future::Future;
use std::sync::Arc;

use anyhow::Result;
use futures::future::join_all;
use serde_json::Value;

use crate::api::client::CxClient;
use crate::config::ResolvedConfig;

// ── Execution target ──────────────────────────────────────────────────────────

/// A fully resolved, ready-to-use execution context for one profile.
///
/// Owns the resolved config *and* the pre-built HTTP client so that
/// downstream code never needs to construct a `CxClient` manually.
pub struct ExecutionTarget {
    pub profile_name: String,
    pub cfg: ResolvedConfig,
    pub client: CxClient,
}

impl ExecutionTarget {
    /// Build an `ExecutionTarget` from an already-resolved config.
    pub fn new(cfg: ResolvedConfig) -> Result<Self> {
        let client = CxClient::new(&cfg.endpoint, &cfg.api_key)?;
        let profile_name = cfg.profile_name.clone();
        Ok(Self { profile_name, cfg, client })
    }
}

/// Build a list of `ExecutionTarget`s from a list of resolved configs.
pub fn build_targets(configs: Vec<ResolvedConfig>) -> Result<Vec<Arc<ExecutionTarget>>> {
    configs.into_iter().map(|cfg| ExecutionTarget::new(cfg).map(Arc::new)).collect()
}

// ── Fan-out ───────────────────────────────────────────────────────────────────

/// Run `f` concurrently for every target and collect the results.
///
/// Returns a `Vec` of `(profile_name, Result<T>)` in completion order.
/// Errors are preserved per-profile so the caller can report failures
/// while still rendering results from the profiles that succeeded.
pub async fn fan_out<T, F, Fut>(
    targets: &[Arc<ExecutionTarget>],
    f: F,
) -> Vec<(String, Result<T>)>
where
    F: Fn(Arc<ExecutionTarget>) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let futures = targets.iter().map(|t| {
        let profile_name = t.profile_name.clone();
        let fut = f(Arc::clone(t));
        async move { (profile_name, fut.await) }
    });
    join_all(futures).await
}

// ── Row tagging ───────────────────────────────────────────────────────────────

/// Inject a `"profile"` key into each JSON object row when `include_profile` is true.
///
/// Non-object values are passed through unchanged so callers don't need to
/// guard against unexpected shapes.
pub fn tag_rows(rows: Vec<Value>, profile: &str, include_profile: bool) -> Vec<Value> {
    if !include_profile {
        return rows;
    }
    rows.into_iter()
        .map(|mut row| {
            if let Value::Object(ref mut m) = row {
                m.insert("profile".to_string(), Value::String(profile.to_string()));
            }
            row
        })
        .collect()
}

/// Merge per-profile results into a single flat list of optionally tagged rows.
///
/// Errors are printed to stderr; successful results are tagged with
/// their source `profile_name` (when `include_profile` is true) and appended
/// in profile order.
pub fn merge_tagged_results(
    per_profile: Vec<(String, Result<Vec<Value>>)>,
    include_profile: bool,
) -> Vec<Value> {
    let mut all: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(rows) => all.extend(tag_rows(rows, &profile, include_profile)),
            Err(e) => eprintln!("error from profile '{profile}': {e:#}"),
        }
    }
    all
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tag_rows_injects_profile_key_when_enabled() {
        let rows = vec![
            json!({"timestamp": "2024-01-01T00:00:00Z", "message": "hello"}),
            json!({"timestamp": "2024-01-01T00:00:01Z", "message": "world"}),
        ];
        let tagged = tag_rows(rows, "prod", true);
        for row in &tagged {
            assert_eq!(row["profile"], json!("prod"));
        }
    }

    #[test]
    fn tag_rows_skips_profile_when_disabled() {
        let rows = vec![
            json!({"timestamp": "2024-01-01T00:00:00Z", "message": "hello"}),
        ];
        let tagged = tag_rows(rows.clone(), "prod", false);
        assert!(tagged[0].get("profile").is_none());
        assert_eq!(tagged[0]["message"], json!("hello"));
    }

    #[test]
    fn tag_rows_passes_non_object_values_through() {
        let rows = vec![json!("plain string"), json!(42)];
        let tagged = tag_rows(rows.clone(), "p", true);
        assert_eq!(tagged[0], json!("plain string"));
        assert_eq!(tagged[1], json!(42));
    }

    #[test]
    fn merge_tagged_results_combines_rows_with_profile_when_enabled() {
        let per_profile = vec![
            ("prod".to_string(), Ok(vec![json!({"a": 1}), json!({"a": 2})])),
            ("staging".to_string(), Ok(vec![json!({"a": 3})])),
        ];
        let merged = merge_tagged_results(per_profile, true);

        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0]["profile"], json!("prod"));
        assert_eq!(merged[1]["profile"], json!("prod"));
        assert_eq!(merged[2]["profile"], json!("staging"));
    }

    #[test]
    fn merge_tagged_results_omits_profile_when_disabled() {
        let per_profile = vec![
            ("prod".to_string(), Ok(vec![json!({"a": 1})])),
        ];
        let merged = merge_tagged_results(per_profile, false);

        assert_eq!(merged.len(), 1);
        assert!(merged[0].get("profile").is_none());
        assert_eq!(merged[0]["a"], json!(1));
    }

    #[test]
    fn merge_tagged_results_skips_errored_profiles() {
        let per_profile: Vec<(String, Result<Vec<Value>>)> = vec![
            ("good".to_string(), Ok(vec![json!({"a": 1})])),
            ("bad".to_string(), Err(anyhow::anyhow!("simulated error"))),
            ("also-good".to_string(), Ok(vec![json!({"a": 2})])),
        ];
        let merged = merge_tagged_results(per_profile, true);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0]["profile"], json!("good"));
        assert_eq!(merged[1]["profile"], json!("also-good"));
    }

    #[test]
    fn tag_rows_overwrites_existing_profile_key() {
        let rows = vec![json!({"profile": "old", "data": "x"})];
        let tagged = tag_rows(rows, "new", true);
        assert_eq!(tagged[0]["profile"], json!("new"));
    }
}
