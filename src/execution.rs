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
use crate::identity;
use crate::request_metadata::RequestMetadata;

// ── Execution target ──────────────────────────────────────────────────────────

/// A fully resolved, ready-to-use execution context for one profile.
///
/// Owns the resolved config *and* the pre-built HTTP client so that
/// downstream code never needs to construct a `CxClient` manually.
pub struct ExecutionTarget {
    pub profile_name: String,
    pub cfg: ResolvedConfig,
    pub client: CxClient,
    /// Lazily-resolved console base URL, cached in-process so repeated
    /// console-link lookups within one command invocation don't re-hit
    /// `/identity/whoami`. Backed by a longer-lived on-disk cache in the
    /// profile file (see `config::load_cached_console_url`) so that most
    /// invocations skip the network lookup entirely.
    console_base: OnceCell<Option<String>>,
    /// When true, [`console_base`](Self::console_base) always returns `None`,
    /// suppressing every "View in Coralogix" stderr link for this target. Set
    /// from the `--no-console-link` flag / `CX_NO_CONSOLE_LINK` env var /
    /// `no_console_link` config key via [`build_targets`]. Defaults to
    /// `false` here so direct `new()` callers (tests) are unaffected.
    no_console_link: bool,
    pub request_metadata: RequestMetadata,
}

impl ExecutionTarget {
    /// Build an `ExecutionTarget` from an already-resolved config.
    pub fn new(cfg: ResolvedConfig, request_metadata: RequestMetadata) -> Result<Self> {
        let client = CxClient::new_with_metadata(&cfg.endpoint, &cfg.api_key, &request_metadata)?;
        let profile_name = cfg.profile_name.clone();
        Ok(Self {
            profile_name,
            cfg,
            client,
            console_base: OnceCell::new(),
            no_console_link: false,
            request_metadata,
        })
    }

    /// Resolve the base URL to build "View in Coralogix" console links
    /// against.
    ///
    /// Resolution order:
    /// 1. An explicit `console_url` configured on the profile TOML - used
    ///    as-is (trailing slash already trimmed by `config::resolve_single`).
    ///    Not available to env-only invocations (`CX_API_KEY` + `CX_REGION`
    ///    with no profile file) - there is no `CX_CONSOLE_URL` equivalent, so
    ///    those callers always fall through to steps 2-4.
    /// 2. A fresh on-disk cache of the team's console URL in the profile file
    ///    (see `config::load_cached_console_url`), written by a previous
    ///    invocation. This is what spares the overwhelming majority of
    ///    invocations - and, crucially, agents making many sequential calls -
    ///    the extra `/identity/whoami` round-trip.
    /// 3. Otherwise, the team's console URL resolved automatically via
    ///    `GET /identity/whoami` (see `identity::resolve_team_url`), then
    ///    persisted to the profile cache for next time. This is the default on
    ///    a cold cache: most teams don't need to configure anything at all to
    ///    get console links.
    /// 4. `None` if `/identity/whoami` fails or returns no usable URL.
    ///
    /// When `cfg.credentials_overridden` is set (the bearer token came from a
    /// `--api-key`/`CX_API_KEY` override rather than this profile's own
    /// credentials), steps 1-2 are skipped and step 3's cache write is
    /// skipped: the profile's `console_url` and cached URL belong to a
    /// different team than the one actually in use, and must never be read
    /// or clobbered by this run. Resolution always falls through to a live
    /// `/identity/whoami` call in that case.
    ///
    /// Best-effort and infallible: any lookup (or cache write) failure results
    /// in `None` rather than an error, since a console link is a "nice to
    /// have" that must never fail an otherwise-successful command. Cached per
    /// target so multiple links printed within one invocation only resolve
    /// once.
    ///
    /// Short-circuits to `None` unconditionally when `no_console_link` is
    /// set, before any of the steps above - so `--no-console-link` /
    /// `CX_NO_CONSOLE_LINK` / `no_console_link` in config also skip the
    /// `/identity/whoami` round-trip on a cold cache, not just the printing.
    pub async fn console_base(&self) -> Option<String> {
        if self.no_console_link {
            return None;
        }
        self.console_base
            .get_or_init(|| async {
                if !self.cfg.credentials_overridden {
                    // 1. Explicit override always wins.
                    if let Some(url) = &self.cfg.console_url {
                        return Some(url.clone());
                    }
                    // 2. Fresh on-disk cache - no network.
                    if let Some(url) = crate::config::load_cached_console_url(&self.profile_name) {
                        return Some(url);
                    }
                }
                // 3. Cold cache (or overridden credentials): resolve live.
                let resolved = identity::resolve_team_url(&self.client).await;
                if !self.cfg.credentials_overridden {
                    if let Some(url) = &resolved {
                        crate::config::cache_console_url(&self.profile_name, url);
                    }
                }
                resolved
            })
            .await
            .clone()
    }

    /// Resolve this target's console base URL, build a "View in Coralogix"
    /// link with `build`, print it to stderr, and return it.
    ///
    /// This is the single place that ties `console_base` resolution to
    /// printing: every command that prints a console link should go through
    /// this method (or [`console_link_for_profile`] when working from a
    /// `(profile_name, T)` pair rather than an `ExecutionTarget` directly)
    /// instead of re-deriving the base/build/print sequence inline, so
    /// there's exactly one place that can forget to print.
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
/// execution::console_link_for_profile(targets, &profile, |b| {
///     console_url::alert_url(b, &id)
/// })
/// .await;
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
///
/// `no_console_link` is applied to every target so all "View in Coralogix"
/// links (see [`ExecutionTarget::console_base`]) are suppressed uniformly
/// across a fan-out.
pub fn build_targets(
    configs: Vec<ResolvedConfig>,
    request_metadata: RequestMetadata,
    no_console_link: bool,
) -> Result<Vec<Arc<ExecutionTarget>>> {
    configs
        .into_iter()
        .map(|cfg| {
            let mut target = ExecutionTarget::new(cfg, request_metadata.clone())?;
            target.no_console_link = no_console_link;
            Ok(Arc::new(target))
        })
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
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_cfg(endpoint: &str, console_url: Option<&str>) -> ResolvedConfig {
        ResolvedConfig {
            profile_name: "test-profile".to_string(),
            api_key: "test-key".to_string(),
            auth_kind: crate::config::AuthKind::ApiKey,
            endpoint: endpoint.to_string(),
            default_tier: crate::Tier::Archive,
            console_url: console_url.map(str::to_string),
            credentials_overridden: false,
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
        let a = Arc::new(
            ExecutionTarget::new(
                test_cfg("http://127.0.0.1:1", None),
                RequestMetadata::default(),
            )
            .unwrap(),
        );
        let targets = vec![a.clone()];
        assert!(find_target(&targets, "test-profile").is_some());
        assert!(find_target(&targets, "missing-profile").is_none());
    }

    #[tokio::test]
    async fn console_base_prefers_explicit_console_url() {
        install_rustls_provider();
        // No mock server is started for this test, so a whoami call would
        // fail the test outright (connection refused) - the explicit
        // console_url must short-circuit before that.
        let target = ExecutionTarget::new(
            test_cfg("http://127.0.0.1:1", Some("https://c4c.example.com/")),
            RequestMetadata::default(),
        )
        .unwrap();
        assert_eq!(
            target.console_base().await,
            Some("https://c4c.example.com/".to_string())
        );
    }

    #[tokio::test]
    async fn console_base_uses_team_url_from_whoami_by_default() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "team_id": 1,
                "team_name": "C4C",
                "team_url": "https://c4c.app.eu2.coralogix.com"
            })))
            .mount(&server)
            .await;

        // No explicit console_url - must resolve via /identity/whoami by
        // default.
        let target =
            ExecutionTarget::new(test_cfg(&server.uri(), None), RequestMetadata::default())
                .unwrap();
        assert_eq!(
            target.console_base().await,
            Some("https://c4c.app.eu2.coralogix.com".to_string())
        );
    }

    #[tokio::test]
    async fn console_base_none_when_whoami_omits_team_url() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "team_id": 1,
                "team_name": "C4C"
            })))
            .mount(&server)
            .await;

        let target =
            ExecutionTarget::new(test_cfg(&server.uri(), None), RequestMetadata::default())
                .unwrap();
        assert_eq!(target.console_base().await, None);
    }

    #[tokio::test]
    async fn console_base_none_when_whoami_fails() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let target =
            ExecutionTarget::new(test_cfg(&server.uri(), None), RequestMetadata::default())
                .unwrap();
        // Best-effort: a failing /identity/whoami must not error the caller,
        // just result in no console link.
        assert_eq!(target.console_base().await, None);
    }

    #[tokio::test]
    async fn console_base_explicit_console_url_takes_precedence_over_whoami() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "team_id": 1,
                "team_name": "Totally Different Name",
                "team_url": "https://totally-different.app.eu2.coralogix.com"
            })))
            .expect(0)
            .mount(&server)
            .await;

        // An explicit console_url must win outright - whoami must not even
        // be called (wiremock's `.expect(0)` above enforces this at drop
        // time).
        let target = ExecutionTarget::new(
            test_cfg(&server.uri(), Some("https://c4c.app.eu2.coralogix.com")),
            RequestMetadata::default(),
        )
        .unwrap();
        assert_eq!(
            target.console_base().await,
            Some("https://c4c.app.eu2.coralogix.com".to_string())
        );
    }

    #[tokio::test]
    async fn console_base_is_cached_after_first_resolution() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "team_id": 1,
                "team_name": "C4C",
                "team_url": "https://c4c.app.eu2.coralogix.com"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let target =
            ExecutionTarget::new(test_cfg(&server.uri(), None), RequestMetadata::default())
                .unwrap();
        // Two calls must only hit /identity/whoami once (wiremock's `.expect(1)`
        // above would fail the mock verification otherwise).
        let first = target.console_base().await;
        let second = target.console_base().await;
        assert_eq!(first, second);
        assert_eq!(first, Some("https://c4c.app.eu2.coralogix.com".to_string()));
    }

    #[tokio::test]
    async fn console_base_none_when_no_console_link_set_even_with_explicit_console_url() {
        install_rustls_provider();
        // No mock server is started for this test either - if `no_console_link`
        // didn't short-circuit before the explicit `console_url` check, this
        // test would still pass (explicit URL wins), so the real assertion is
        // that suppression overrides *everything*, not just the whoami path.
        let mut target = ExecutionTarget::new(
            test_cfg("http://127.0.0.1:1", Some("https://c4c.example.com/")),
            RequestMetadata::default(),
        )
        .unwrap();
        target.no_console_link = true;
        assert_eq!(target.console_base().await, None);
    }

    #[test]
    fn build_targets_propagates_no_console_link_to_every_target() {
        install_rustls_provider();
        let configs = vec![
            test_cfg("http://127.0.0.1:1", Some("https://c4c.example.com/")),
            test_cfg("http://127.0.0.1:2", Some("https://other.example.com/")),
        ];
        let targets = build_targets(configs, RequestMetadata::default(), true).unwrap();
        assert_eq!(targets.len(), 2);
        for target in &targets {
            assert!(target.no_console_link);
        }
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
