//! HTTP client for the cx-cli onboarding service.
//!
//! A single `POST /api/v2/onboarding` marking the caller as onboarded: no body,
//! `204` on success, idempotent. Identity comes from the auth context the
//! gateway injects. Best-effort — the caller swallows any error (see
//! `report_onboarding` in `mod.rs`).

use anyhow::Result;
use serde_json::Value;

use crate::api_client::CxClient;

/// Gateway path for the onboarding endpoint. The service rewrites this prefix
/// to `/onboarding` internally; callers use the `/api/v2` gateway path.
const ONBOARDING_BASE: &str = "/api/v2/onboarding";

/// Record the calling user as onboarded via `POST /api/v2/onboarding`.
///
/// The endpoint returns an empty `204` body, so the response is discarded.
/// Callers treat this as best-effort — a failure here (service not yet deployed
/// to the region, a non-user token, a transient error) must never fail `cx
/// init`, since credential verification is owned by the `whoami` probe.
pub async fn report_onboarded(client: &CxClient) -> Result<()> {
    // `post_empty` sends no body and tolerates the empty 204 response.
    let _: Value = client.post_empty(ONBOARDING_BASE, &[]).await?;
    Ok(())
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

    #[tokio::test]
    async fn report_onboarded_succeeds_on_204() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/onboarding"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = CxClient::new(server.uri(), "test-key").unwrap();
        report_onboarded(&client).await.expect("204 should be Ok");
    }

    /// A non-user token is rejected with 403; the caller swallows this, but the
    /// function itself surfaces the error.
    #[tokio::test]
    async fn report_onboarded_errors_on_403() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/onboarding"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let client = CxClient::new(server.uri(), "team-key").unwrap();
        report_onboarded(&client)
            .await
            .expect_err("403 should be an error");
    }

    /// The service not being deployed to a region surfaces as a 404; again an
    /// error the caller is expected to swallow.
    #[tokio::test]
    async fn report_onboarded_errors_on_404() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/onboarding"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = CxClient::new(server.uri(), "test-key").unwrap();
        report_onboarded(&client)
            .await
            .expect_err("404 should be an error");
    }
}
