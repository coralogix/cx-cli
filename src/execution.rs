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
    /// lookups within one command invocation don't re-hit `/identity/whoami`.
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
    ///    `https://<console_team_name>.<console_domain>` - skips step 3
    ///    entirely.
    /// 3. A known `console_domain`, combined with the team subdomain
    ///    resolved automatically via `GET /identity/whoami` (see
    ///    `identity::resolve_team_subdomain`). This is the default: most
    ///    teams don't need to configure anything at all to get console
    ///    links.
    /// 4. `None` - the region has no known console domain (e.g.
    ///    `Region::Custom`), or no subdomain could be resolved by either of
    ///    the above.
    ///
    /// Best-effort and infallible: any lookup failure results in `None`
    /// rather than an error, since a console link is a "nice to have" that
    /// must never fail an otherwise-successful command. Cached per target so
    /// multiple links printed within one invocation only hit
    /// `/identity/whoami` once.
    pub async fn console_base(&self) -> Option<String> {
        self.console_base
            .get_or_init(|| async {
                if let Some(url) = &self.cfg.console_url {
                    return Some(url.clone());
                }
                let domain = self.cfg.console_domain.as_deref()?;
                if let Some(team) = self.cfg.console_team_name.as_deref() {
                    return Some(format!("https://{team}.{domain}"));
                }
                let subdomain = identity::resolve_team_subdomain(&self.client).await?;
                Some(format!("https://{subdomain}.{domain}"))
            })
            .await
            .clone()
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

/// Resolve `profile`'s console link via [`console_link_for_profile`] and, if
/// one was found, embed it into `val` via `render::tag_console_url`.
///
/// Collapses the `console_link_for_profile` + `render::tag_console_url`
/// idiom that used to be repeated inline at every call site across command
/// modules that print a "View in Coralogix" link and *only* tag a single
/// result value with it - which is the overwhelming majority of those call
/// sites (~100 of them at the time this was extracted). A handful of call
/// sites still use `console_link_for_profile` directly instead of this
/// helper, because they need the URL for more than just tagging one value
/// (e.g. also embedding it in text-mode output) - those are intentionally
/// left alone.
///
/// No-op (silently - a console link is always best-effort) if `profile` has
/// no matching target, or if no console base URL could be resolved for it.
pub async fn tag_console_link_for_profile(
    targets: &[Arc<ExecutionTarget>],
    profile: &str,
    val: &mut serde_json::Value,
    build: impl FnOnce(&str) -> String,
) {
    if let Some(url) = console_link_for_profile(targets, profile, build).await {
        crate::render::tag_console_url(val, &url);
    }
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
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
        // No console_domain set, so a whoami call would panic this test if
        // attempted - the explicit console_url must short-circuit before that.
        let target = ExecutionTarget::new(test_cfg(
            "http://127.0.0.1:1",
            Some("https://acme.example.com/"),
            None,
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
        // An explicit console_team_name must skip /identity/whoami entirely -
        // no mock server is even started for this test, so any attempted
        // call would fail the test outright (connection refused).
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
    async fn console_base_none_when_domain_missing() {
        install_rustls_provider();
        // An explicit console_team_name alone isn't enough without a known
        // console_domain for the region (e.g. Region::Custom) - and with no
        // domain, whoami is never attempted either.
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
    async fn console_base_combines_domain_and_team_subdomain_from_whoami() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "team_id": 1,
                "team_name": "Acme Corp",
                "team_url": "acme"
            })))
            .mount(&server)
            .await;

        // No explicit console_team_name - must resolve via /identity/whoami
        // by default.
        let target =
            ExecutionTarget::new(test_cfg(&server.uri(), None, Some("app.eu2.coralogix.com")))
                .unwrap();
        assert_eq!(
            target.console_base().await,
            Some("https://acme.app.eu2.coralogix.com".to_string())
        );
    }

    #[tokio::test]
    async fn console_base_falls_back_to_team_name_when_whoami_omits_team_url() {
        install_rustls_provider();
        let server = MockServer::start().await;
        // Confirmed against a live team during FORGE-586 review: team_url is
        // genuinely absent from some real teams' /identity/whoami responses,
        // even though team_name is present and perfectly usable - so this
        // must resolve via the sanitized team_name guess by default, not
        // fail through to None.
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "team_id": 1,
                "team_name": "Acme Corp"
            })))
            .mount(&server)
            .await;

        let target =
            ExecutionTarget::new(test_cfg(&server.uri(), None, Some("app.eu2.coralogix.com")))
                .unwrap();
        assert_eq!(
            target.console_base().await,
            Some("https://acme-corp.app.eu2.coralogix.com".to_string())
        );
    }

    #[tokio::test]
    async fn console_base_none_when_whoami_omits_both_team_url_and_team_name() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "team_id": 1 })),
            )
            .mount(&server)
            .await;

        let target =
            ExecutionTarget::new(test_cfg(&server.uri(), None, Some("app.eu2.coralogix.com")))
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
            ExecutionTarget::new(test_cfg(&server.uri(), None, Some("app.eu2.coralogix.com")))
                .unwrap();
        // Best-effort: a failing /identity/whoami must not error the caller,
        // just result in no console link.
        assert_eq!(target.console_base().await, None);
    }

    #[tokio::test]
    async fn console_base_explicit_team_name_takes_precedence_over_whoami() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "team_id": 1,
                "team_name": "Totally Different Name",
                "team_url": "totally-different"
            })))
            .expect(0)
            .mount(&server)
            .await;

        // An explicit console_team_name must win outright - whoami must not
        // even be called (wiremock's `.expect(0)` above enforces this at
        // drop time).
        let target = ExecutionTarget::new(test_cfg_with_team_name(
            &server.uri(),
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
    async fn console_base_is_cached_after_first_resolution() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "team_id": 1,
                "team_name": "Acme Corp",
                "team_url": "acme"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let target =
            ExecutionTarget::new(test_cfg(&server.uri(), None, Some("app.eu2.coralogix.com")))
                .unwrap();
        // Two calls must only hit /identity/whoami once (wiremock's `.expect(1)`
        // above would fail the mock verification otherwise).
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
