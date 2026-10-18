//! Best-effort "this user is on the CLI" report to the cx-cli onboarding service.
//!
//! A single `POST /api/v2/onboarding` marking the caller as onboarded: no body,
//! `204` on success, idempotent. Identity comes from the auth context the
//! gateway injects. The web app's CLI-connect dialog polls the service to show
//! "connected".
//!
//! Reported from:
//! * `cx profiles add` and `cx init` — always ([`report`]).
//! * Any API command, until the profile's report has gone through once
//!   ([`report_pending`]). This covers profiles set up before the CLI reported
//!   at all, and agents that skip `cx init` because `cx` is already installed.
//!
//! Profiles whose report went through are listed in `~/.cx/onboarding.json`
//! (managed by `cx`), so a settled profile costs no request.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use serde_json::Value;

use crate::api_client::CxClient;
use crate::error::{CxError, Result};
use crate::execution::ExecutionTarget;
use crate::region::{region_from_url, RegionMatch};

/// Gateway path for the onboarding endpoint. The service rewrites this prefix
/// to `/onboarding` internally; callers use the `/api/v2` gateway path.
const ONBOARDING_BASE: &str = "/api/v2/onboarding";

/// Bound on a report: the shared client carries no request timeout, so a
/// gateway that accepts the connection but stalls the response would
/// otherwise hang the command. A stall is swallowed like any other failure.
const REPORT_TIMEOUT: Duration = Duration::from_secs(5);

/// Record the calling user as onboarded via `POST /api/v2/onboarding`.
///
/// The endpoint returns an empty `204` body, so the response is discarded.
pub async fn report_onboarded(client: &CxClient) -> Result<()> {
    // `post_empty` sends no body and tolerates the empty 204 response.
    let _: Value = client.post_empty(ONBOARDING_BASE, &[]).await?;
    Ok(())
}

/// How a report ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The service recorded the user.
    Reported,
    /// `403`: a non-user identity (e.g. a team API key) the service will never
    /// accept.
    Forbidden,
    /// Anything else (service not yet deployed to the region, network error,
    /// timeout) — retried on the next command.
    Failed,
}

impl Outcome {
    fn from_result(result: &Result<()>) -> Self {
        match result {
            Ok(()) => Self::Reported,
            Err(CxError::Permission(_)) => Self::Forbidden,
            Err(_) => Self::Failed,
        }
    }

    /// Whether the profile needs no further reports. A `403` settles too —
    /// otherwise every command would retry it.
    fn is_settled(self) -> bool {
        self != Self::Failed
    }
}

/// Report `profile` now, whether or not it already went through. Best-effort:
/// never fails the calling command. `None` when skipped (custom / BYOC
/// endpoint).
pub async fn report(profile: &str, endpoint: &str, api_key: &str) -> Option<Outcome> {
    let outcome = send(endpoint, api_key).await?;
    let mut state = ReportState::load();
    state.set(profile, outcome.is_settled());
    state.save();
    Some(outcome)
}

/// Report every target whose profile hasn't gone through yet. Silent and
/// best-effort.
pub async fn report_pending(targets: Vec<Arc<ExecutionTarget>>) {
    let pending: Vec<_> = {
        let state = ReportState::load();
        targets
            .into_iter()
            .filter(|t| !state.is_settled(&t.profile_name))
            .collect()
    };
    if pending.is_empty() {
        return;
    }

    let outcomes = join_all(pending.iter().map(|t| async {
        let outcome = send(&t.cfg.endpoint, &t.cfg.api_key).await;
        (t.profile_name.as_str(), outcome)
    }))
    .await;

    // One load-modify-save, so concurrent reports don't overwrite each other.
    let mut state = ReportState::load();
    for (profile, outcome) in outcomes {
        if let Some(outcome) = outcome {
            state.set(profile, outcome.is_settled());
        }
    }
    state.save();
}

/// Send one report. `None` when skipped: custom / BYOC endpoints may not route
/// the onboarding service.
async fn send(endpoint: &str, api_key: &str) -> Option<Outcome> {
    if matches!(region_from_url(endpoint), RegionMatch::Unresolved) {
        return None;
    }
    let client = CxClient::new(endpoint, api_key).ok()?;
    let outcome = match tokio::time::timeout(REPORT_TIMEOUT, report_onboarded(&client)).await {
        Ok(result) => Outcome::from_result(&result),
        Err(_) => Outcome::Failed,
    };
    Some(outcome)
}

/// Profiles whose report has settled.
#[derive(Debug, Default)]
struct ReportState {
    settled: BTreeSet<String>,
}

impl ReportState {
    fn path() -> Option<PathBuf> {
        crate::config::config_dir()
            .ok()
            .map(|d| d.join("onboarding.json"))
    }

    /// Load from disk, returning an empty state on any error.
    fn load() -> Self {
        let settled = Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default();
        Self { settled }
    }

    /// Persist to disk. Errors are swallowed — the worst case is an extra report.
    fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(contents) = serde_json::to_string_pretty(&self.settled) {
            let _ = std::fs::write(path, contents);
        }
    }

    fn is_settled(&self, profile: &str) -> bool {
        self.settled.contains(profile)
    }

    /// An unsettled outcome clears the profile, so a re-pointed profile whose
    /// report failed (e.g. `cx profiles add --force`) is retried.
    fn set(&mut self, profile: &str, settled: bool) {
        if settled {
            self.settled.insert(profile.to_string());
        } else {
            self.settled.remove(profile);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn install_rustls_provider() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
    }

    async fn report_with_status(status: u16) -> Result<()> {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/onboarding"))
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount(&server)
            .await;

        let client = CxClient::new(server.uri(), "test-key").unwrap();
        report_onboarded(&client).await
    }

    #[tokio::test]
    async fn report_onboarded_succeeds_on_204() {
        report_with_status(204).await.expect("204 should be Ok");
    }

    async fn outcome_for(status: u16) -> Outcome {
        Outcome::from_result(&report_with_status(status).await)
    }

    #[tokio::test]
    async fn a_204_is_reported_and_settles_the_profile() {
        let outcome = outcome_for(204).await;
        assert_eq!(outcome, Outcome::Reported);
        assert!(outcome.is_settled());
    }

    /// A non-user token (team key) is rejected with 403 and will never pass.
    #[tokio::test]
    async fn a_403_is_forbidden_and_settles_the_profile() {
        let outcome = outcome_for(403).await;
        assert_eq!(outcome, Outcome::Forbidden);
        assert!(outcome.is_settled());
    }

    /// The service not being deployed to a region surfaces as a 404 — retried.
    #[tokio::test]
    async fn a_404_leaves_the_profile_pending() {
        assert!(!outcome_for(404).await.is_settled());
    }

    #[tokio::test]
    async fn a_5xx_leaves_the_profile_pending() {
        assert!(!outcome_for(503).await.is_settled());
    }

    #[test]
    fn state_tracks_settled_profiles_individually() {
        let mut state = ReportState::default();
        state.set("default", true);
        assert!(state.is_settled("default"));
        assert!(!state.is_settled("staging"));
    }

    #[test]
    fn an_unsettled_outcome_clears_a_settled_profile() {
        let mut state = ReportState::default();
        state.set("default", true);
        state.set("default", false);
        assert!(!state.is_settled("default"));
    }
}
