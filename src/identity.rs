use anyhow::anyhow;
use serde::Deserialize;

use crate::api_client::CxClient;

const WHOAMI_BASE: &str = "/identity/whoami";

/// Response from `GET /identity/whoami`. Identifies the team (and caller)
/// that the current API key belongs to. Payload uses snake_case field names.
#[derive(Debug, Deserialize)]
pub struct Whoami {
    pub team_id: Option<i64>,
    pub team_name: Option<String>,
    pub user_name: Option<String>,
    /// The team's web console base URL, e.g.
    /// `"https://my-team.app.eu2.coralogix.com"`. Used as-is (see
    /// [`resolve_team_url`]) to build "View in Coralogix" console links.
    #[serde(default)]
    pub team_url: Option<String>,
}

/// Resolve the team ID for the current API key via `GET /identity/whoami`.
///
/// Preferred over the SAML config endpoint because `/identity/whoami` is
/// readable by every API key regardless of scopes, and works on teams that
/// have not configured SAML.
pub async fn resolve_team_id(client: &CxClient) -> anyhow::Result<String> {
    let whoami: Whoami = client
        .get(WHOAMI_BASE, &[])
        .await
        .map_err(|e| anyhow!("failed to resolve team ID via /identity/whoami: {e:#}"))?;
    whoami
        .team_id
        .map(|id| id.to_string())
        .ok_or_else(|| anyhow!("/identity/whoami returned no team_id"))
}

/// Resolve the team's web console base URL used to build "View in
/// Coralogix" console links, via `GET /identity/whoami`. This is the
/// default automatic resolution path, used whenever a profile hasn't set an
/// explicit `console_url` override (see `docs/configuration.md#console-links`).
///
/// Returns `whoami.team_url` verbatim, with any trailing slash trimmed.
///
/// Best-effort: any failure (network, auth, missing/empty field) returns
/// `None` rather than an error, since a console link is a "nice to have" -
/// it must never cause an otherwise-successful create/edit command to fail.
pub async fn resolve_team_url(client: &CxClient) -> Option<String> {
    let whoami: Whoami = client.get(WHOAMI_BASE, &[]).await.ok()?;
    let url = whoami.team_url?;
    let trimmed = url.trim_end_matches('/');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
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

    #[test]
    fn deserialize_whoami() {
        let json = serde_json::json!({ "team_id": 53623, "team_name": "c4c", "user_name": "alice@example.com" });
        let w: Whoami = serde_json::from_value(json).unwrap();
        assert_eq!(w.team_id, Some(53623));
        assert_eq!(w.team_name.as_deref(), Some("c4c"));
    }

    #[test]
    fn deserialize_whoami_minimal() {
        let json = serde_json::json!({ "team_id": 12345 });
        let w: Whoami = serde_json::from_value(json).unwrap();
        assert_eq!(w.team_id, Some(12345));
        assert!(w.team_name.is_none());
    }

    #[test]
    fn deserialize_whoami_with_team_url() {
        let json = serde_json::json!({
            "team_id": 12345,
            "team_name": "C4C",
            "team_url": "https://c4c.app.eu2.coralogix.com"
        });
        let w: Whoami = serde_json::from_value(json).unwrap();
        assert_eq!(
            w.team_url.as_deref(),
            Some("https://c4c.app.eu2.coralogix.com")
        );
    }

    #[tokio::test]
    async fn resolve_team_url_returns_team_url_verbatim() {
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

        let client = CxClient::new(server.uri(), "test-key").unwrap();
        assert_eq!(
            resolve_team_url(&client).await,
            Some("https://c4c.app.eu2.coralogix.com".to_string())
        );
    }

    #[tokio::test]
    async fn resolve_team_url_trims_trailing_slash() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "team_id": 1,
                "team_url": "https://c4c.app.eu2.coralogix.com/"
            })))
            .mount(&server)
            .await;

        let client = CxClient::new(server.uri(), "test-key").unwrap();
        assert_eq!(
            resolve_team_url(&client).await,
            Some("https://c4c.app.eu2.coralogix.com".to_string())
        );
    }

    #[tokio::test]
    async fn resolve_team_url_none_when_team_url_absent() {
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

        let client = CxClient::new(server.uri(), "test-key").unwrap();
        assert_eq!(resolve_team_url(&client).await, None);
    }

    #[tokio::test]
    async fn resolve_team_url_none_when_whoami_fails() {
        install_rustls_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/identity/whoami"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = CxClient::new(server.uri(), "test-key").unwrap();
        assert_eq!(resolve_team_url(&client).await, None);
    }
}
