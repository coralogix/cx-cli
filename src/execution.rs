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
use tokio::sync::OnceCell;

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
    /// Lazily-resolved console base URL (e.g.
    /// `https://acme.app.eu2.coralogix.com`), cached so repeated console-link
    /// lookups within one command invocation only build the string once.
    console_base: OnceCell<Option<String>>,
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
            console_base: OnceCell::new(),
        })
    }

    /// Resolve the base URL to build "View in Coralogix" console links
    /// against, e.g. `https://acme.app.eu2.coralogix.com`.
    ///
    /// Resolution order:
    /// 1. An explicit `console_url` configured on the profile/env - used
    ///    as-is (trailing slash already trimmed by `config::resolve_single`).
    /// 2. A known `console_domain` for the profile's region, combined with
    ///    an explicit `console_team_name` configured on the profile, e.g.
    ///    `https://<console_team_name>.<console_domain>`.
    /// 3. `None` - the region has no known console domain (e.g.
    ///    `Region::Custom`), or `console_team_name` was not set.
    ///
    /// There is no API call involved: both `console_url` and
    /// `console_team_name` are purely user-supplied config values, so
    /// resolution is synchronous and infallible. A console link is a "nice
    /// to have" that must never fail an otherwise-successful command, so
    /// step 3 falls back to `None` rather than an error. Cached per target
    /// (via `OnceCell`) purely to print the "unavailable" hint at most once
    /// per invocation - the underlying computation is cheap either way.
    pub async fn console_base(&self) -> Option<String> {
        self.console_base
            .get_or_init(|| async {
                let resolved = self.resolve_console_base_from_config();
                // Only reached once per target - `get_or_init` runs its
                // closure at most once - so this can't spam stderr across
                // repeated console-link lookups within one invocation.
                if resolved.is_none() {
                    crate::render::print_console_link_unavailable_hint();
                }
                resolved
            })
            .await
            .clone()
    }

    /// Synchronous resolution of `console_base` from config alone - see its
    /// doc comment for the resolution order. Split out purely so
    /// `console_base` can print its one-time hint when this comes back
    /// empty, without duplicating the hint check at every early-return
    /// inside this chain.
    fn resolve_console_base_from_config(&self) -> Option<String> {
        if let Some(url) = &self.cfg.console_url {
            return Some(url.clone());
        }
        let domain = self.cfg.console_domain.as_deref()?;
        let team = self.cfg.console_team_name.as_deref()?;
        Some(format!("https://{team}.{domain}"))
    }

    /// Resolve this target's console base URL, build a "View in Coralogix"
    /// link with `build`, print it to stderr, and return it - so callers can
    /// also embed it in `-o json` / `-o agents` output via
    /// `render::tag_console_url`.
    ///
    /// This is the single place that ties `console_base` resolution to
    /// printing: every command that prints a console link should go through
    /// this method (or [`console_link_for_profile`] when working from a
    /// `(profile_name, T)` pair rather than an `ExecutionTarget` directly)
    /// instead of re-deriving the base/build/print/return sequence inline, so
    /// there's exactly one place that can forget to print, or print without
    /// returning a value to tag.
    ///
    /// Returns `None` (silently - a console link is always best-effort) if no
    /// console base URL could be resolved for this target.
    pub async fn console_link(&self, build: impl FnOnce(&str) -> String) -> Option<String> {
        let base = self.console_base().await?;
        let url = build(&base);
        crate::render::print_console_link(&url);
        Some(url)
    }
}

/// Look up `profile`'s target in `targets`, then build+print+return its
/// console link via [`ExecutionTarget::console_link`].
///
/// Collapses the `find_target` -> `console_base` -> build URL -> print ->
/// return idiom that used to be repeated inline (or reimplemented behind
/// bespoke per-command wrapper functions like `print_dashboard_console_link`)
/// at every call site across command modules that print a "View in
/// Coralogix" link after a fan-out. Callers only need to supply the
/// `console_url::*` builder for their entity:
///
/// ```ignore
/// if let Some(url) = execution::console_link_for_profile(targets, &profile, |b| {
///     console_url::alert_url(b, &id)
/// })
/// .await
/// {
///     render::tag_console_url(&mut val, &url);
/// }
/// ```
///
/// Returns `None` if `profile` has no matching target, or if no console base
/// URL could be resolved for it.
pub async fn console_link_for_profile(
    targets: &[Arc<ExecutionTarget>],
    profile: &str,
    build: impl FnOnce(&str) -> String,
) -> Option<String> {
    find_target(targets, profile)?.console_link(build).await
}

/// Build a list of `ExecutionTarget`s from a list of resolved configs.
pub fn build_targets(configs: Vec<ResolvedConfig>) -> Result<Vec<Arc<ExecutionTarget>>> {
    configs
        .into_iter()
        .map(|cfg| ExecutionTarget::new(cfg).map(Arc::new))
        .collect()
}

/// Find the target whose `profile_name` matches `profile`.
///
/// Used after fan-out/fan-in to look a target back up by the profile name
/// carried in the `(profile_name, T)` result pairs - e.g. to resolve a
/// per-profile console link after printing a success line.
pub fn find_target<'a>(
    targets: &'a [Arc<ExecutionTarget>],
    profile: &str,
) -> Option<&'a Arc<ExecutionTarget>> {
    targets.iter().find(|t| t.profile_name == profile)
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
pub fn report_errors_and_collect_successes<T>(
    per_profile: Vec<(String, Result<T>)>,
) -> Result<Vec<(String, T)>> {
    let target_count = per_profile.len();
    let mut successes = Vec::with_capacity(target_count);
    let mut errors: Vec<(String, anyhow::Error)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(v) => successes.push((profile, v)),
            Err(e) => errors.push((profile, e)),
        }
    }

    // Total failure: surface as an error instead of a silently empty result
    // (FORGE-482).
    if successes.is_empty() && target_count > 0 {
        // With a single target there is nothing to aggregate - propagate its
        // actual error so the user sees the real cause, not a generic
        // "all profiles failed" line stacked on top of the printed error.
        if let [_] = errors.as_slice() {
            let (profile, e) = errors.into_iter().next().expect("one error present");
            return Err(e.context(format!("profile '{profile}' failed")));
        }
        for (profile, e) in &errors {
            eprintln!("{}", format!("error from profile '{profile}': {e:#}").red());
        }
        bail!("all {target_count} profiles returned errors");
    }

    // Partial failure: report the failed profiles and return the survivors,
    // per docs/multi-profile.md.
    for (profile, e) in errors {
        eprintln!("{}", format!("error from profile '{profile}': {e:#}").red());
    }
    Ok(successes)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg(
        endpoint: &str,
        console_url: Option<&str>,
        console_domain: Option<&str>,
    ) -> ResolvedConfig {
        test_cfg_with_team_name(endpoint, console_url, console_domain, None)
    }

    fn test_cfg_with_team_name(
        endpoint: &str,
        console_url: Option<&str>,
        console_domain: Option<&str>,
        console_team_name: Option<&str>,
    ) -> ResolvedConfig {
        ResolvedConfig {
            profile_name: "test-profile".to_string(),
            api_key: "test-key".to_string(),
            endpoint: endpoint.to_string(),
            default_tier: crate::Tier::Archive,
            console_url: console_url.map(str::to_string),
            console_domain: console_domain.map(str::to_string),
            console_team_name: console_team_name.map(str::to_string),
        }
    }

    fn install_rustls_provider() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
    }

    #[test]
    fn find_target_locates_by_profile_name() {
        install_rustls_provider();
        let a = Arc::new(ExecutionTarget::new(test_cfg("http://127.0.0.1:1", None, None)).unwrap());
        let targets = vec![a.clone()];
        assert!(find_target(&targets, "test-profile").is_some());
        assert!(find_target(&targets, "missing-profile").is_none());
    }

    #[tokio::test]
    async fn console_base_prefers_explicit_console_url() {
        install_rustls_provider();
        // console_domain/console_team_name are also set here to confirm
        // console_url wins even when both would otherwise produce a link.
        let target = ExecutionTarget::new(test_cfg_with_team_name(
            "http://127.0.0.1:1",
            Some("https://acme.example.com/"),
            Some("app.eu2.coralogix.com"),
            Some("other-team"),
        ))
        .unwrap();
        assert_eq!(
            target.console_base().await,
            Some("https://acme.example.com/".to_string())
        );
    }

    #[tokio::test]
    async fn console_base_none_when_no_domain_and_no_explicit_url() {
        install_rustls_provider();
        // Mirrors Region::Custom: no known console domain, no override -
        // must resolve to None without attempting any HTTP call.
        let target = ExecutionTarget::new(test_cfg("http://127.0.0.1:1", None, None)).unwrap();
        assert_eq!(target.console_base().await, None);
    }

    #[tokio::test]
    async fn console_base_combines_domain_and_explicit_team_name() {
        install_rustls_provider();
        let target = ExecutionTarget::new(test_cfg_with_team_name(
            "http://127.0.0.1:1",
            None,
            Some("app.eu2.coralogix.com"),
            Some("acme"),
        ))
        .unwrap();
        assert_eq!(
            target.console_base().await,
            Some("https://acme.app.eu2.coralogix.com".to_string())
        );
    }

    #[tokio::test]
    async fn console_base_none_when_team_name_missing() {
        install_rustls_provider();
        // A known console domain alone isn't enough - without an explicit
        // console_team_name, there's no team to build a link for, and `cx`
        // must not guess or look one up via any API call.
        let target = ExecutionTarget::new(test_cfg(
            "http://127.0.0.1:1",
            None,
            Some("app.eu2.coralogix.com"),
        ))
        .unwrap();
        assert_eq!(target.console_base().await, None);
    }

    #[tokio::test]
    async fn console_base_none_when_domain_missing() {
        install_rustls_provider();
        // An explicit console_team_name alone isn't enough without a known
        // console_domain for the region (e.g. Region::Custom).
        let target = ExecutionTarget::new(test_cfg_with_team_name(
            "http://127.0.0.1:1",
            None,
            None,
            Some("acme"),
        ))
        .unwrap();
        assert_eq!(target.console_base().await, None);
    }

    #[tokio::test]
    async fn console_base_is_cached_after_first_resolution() {
        install_rustls_provider();
        let target = ExecutionTarget::new(test_cfg_with_team_name(
            "http://127.0.0.1:1",
            None,
            Some("app.eu2.coralogix.com"),
            Some("acme"),
        ))
        .unwrap();
        let first = target.console_base().await;
        let second = target.console_base().await;
        assert_eq!(first, second);
        assert_eq!(
            first,
            Some("https://acme.app.eu2.coralogix.com".to_string())
        );
    }

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
        let successes = report_errors_and_collect_successes(per_profile)
            .expect("partial failure should not bail");
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
        let successes = report_errors_and_collect_successes(per_profile)
            .expect("empty fan-out is not a failure");
        assert!(successes.is_empty());
    }
}
