//! Shared fan-out / fan-in execution layer.
//!
//! This module provides the primitives for running the same command against
//! multiple named profiles concurrently and then combining their results
//! before a single render pass.  The same pattern extends naturally to
//! multi-region fan-out in the future by adding additional target dimensions.

use std::future::Future;
use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use futures::future::join_all;

use crate::api_client::CxClient;
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
        Ok(Self {
            profile_name,
            cfg,
            client,
        })
    }
}

/// Build a list of `ExecutionTarget`s from a list of resolved configs.
pub fn build_targets(configs: Vec<ResolvedConfig>) -> Result<Vec<Arc<ExecutionTarget>>> {
    configs
        .into_iter()
        .map(|cfg| ExecutionTarget::new(cfg).map(Arc::new))
        .collect()
}

// ── Fan-out ───────────────────────────────────────────────────────────────────

/// Run `f` concurrently for every target and collect the results.
///
/// Returns a `Vec` of `(profile_name, Result<T>)` in completion order.
/// Errors are preserved per-profile so the caller can report failures
/// while still rendering results from the profiles that succeeded.
pub async fn fan_out<T, F, Fut>(targets: &[Arc<ExecutionTarget>], f: F) -> Vec<(String, Result<T>)>
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

// ── Error collection ──────────────────────────────────────────────────────────

/// Drain per-profile fan-out results: print every failure to stderr in the
/// standard red format, and return the successful `(profile, value)` pairs in
/// input order.
///
/// Bails when there was at least one target and *all* of them failed, so a
/// total failure surfaces as an error instead of rendering as a silently
/// empty result (FORGE-482). Partial failure (some targets ok) returns `Ok`
/// with the survivors - behavior per docs/multi-profile.md is unchanged.
pub fn report_errors_and_collect_successes<T>(per_profile: Vec<(String, Result<T>)>) -> Result<Vec<(String, T)>> {
    let target_count = per_profile.len();
    let mut successes = Vec::with_capacity(target_count);
    let mut error_count = 0usize;
    for (profile, result) in per_profile {
        match result {
            Ok(v) => successes.push((profile, v)),
            Err(e) => {
                error_count += 1;
                eprintln!("{}", format!("error from profile '{profile}': {e:#}").red());
            }
        }
    }
    if target_count > 0 && error_count == target_count {
        bail!("all profiles returned errors; see above for details");
    }
    Ok(successes)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_successes_returns_all_when_none_fail() {
        let per_profile: Vec<(String, Result<i32>)> =
            vec![("prod".to_string(), Ok(1)), ("staging".to_string(), Ok(2))];
        let successes = report_errors_and_collect_successes(per_profile).expect("should not bail");
        assert_eq!(
            successes,
            vec![("prod".to_string(), 1), ("staging".to_string(), 2)]
        );
    }

    #[test]
    fn collect_successes_skips_errored_profiles_on_partial_failure() {
        let per_profile: Vec<(String, Result<i32>)> = vec![
            ("good".to_string(), Ok(1)),
            ("bad".to_string(), Err(anyhow::anyhow!("simulated error"))),
            ("also-good".to_string(), Ok(2)),
        ];
        let successes = report_errors_and_collect_successes(per_profile).expect("partial failure should not bail");
        assert_eq!(
            successes,
            vec![("good".to_string(), 1), ("also-good".to_string(), 2)]
        );
    }

    #[test]
    fn collect_successes_bails_when_every_target_fails() {
        let per_profile: Vec<(String, Result<i32>)> = vec![
            ("a".to_string(), Err(anyhow::anyhow!("boom"))),
            ("b".to_string(), Err(anyhow::anyhow!("boom"))),
        ];
        let result = report_errors_and_collect_successes(per_profile);
        assert!(
            result.is_err(),
            "a total failure must surface as an error, not a silent empty result"
        );
    }

    #[test]
    fn collect_successes_does_not_bail_on_empty_input() {
        let per_profile: Vec<(String, Result<i32>)> = vec![];
        let successes = report_errors_and_collect_successes(per_profile).expect("empty fan-out is not a failure");
        assert!(successes.is_empty());
    }
}
