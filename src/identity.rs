use anyhow::anyhow;
use serde::Deserialize;

use crate::api_client::CxClient;

const WHOAMI_BASE: &str = "/identity/whoami";

/// Response from `GET /identity/whoami`. Identifies the team (and caller) that
/// the current API key belongs to. Payload uses snake_case field names.
#[derive(Debug, Deserialize)]
pub struct Whoami {
    pub team_id: Option<i64>,
    pub team_name: Option<String>,
    pub user_name: Option<String>,
    /// The team's subdomain label, when the API returns one. Preferred over
    /// `team_name` for building console URLs since a team's display name and
    /// its URL subdomain are not guaranteed to match. Not present on all
    /// `/identity/whoami` responses - callers fall back to `team_name`.
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

/// Extract the hostname-safe team subdomain from a `Whoami` payload.
///
/// Prefers `team_url`, falling back to `team_name`. Only labels made up of
/// lowercase ASCII letters, digits, and hyphens are accepted - anything else
/// (whitespace, unicode, empty string) cannot be a valid hostname label, so
/// this returns `None` rather than build a broken console link.
fn team_subdomain(whoami: &Whoami) -> Option<String> {
    let candidate = whoami.team_url.as_deref().or(whoami.team_name.as_deref())?;
    let lower = candidate.to_lowercase();
    if !lower.is_empty()
        && lower
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        Some(lower)
    } else {
        None
    }
}

/// Resolve the team subdomain used to build "View in Coralogix" console
/// links, e.g. the `acme` in `https://acme.app.eu2.coralogix.com`.
///
/// Best-effort: any failure (network, auth, missing/invalid fields) returns
/// `None` rather than an error, since a console link is a "nice to have" -
/// it must never cause an otherwise-successful create/edit command to fail.
pub async fn resolve_team_subdomain(client: &CxClient) -> Option<String> {
    let whoami: Whoami = client.get(WHOAMI_BASE, &[]).await.ok()?;
    team_subdomain(&whoami)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_whoami() {
        let json = serde_json::json!({ "team_id": 53623, "team_name": "acme", "user_name": "alice@example.com" });
        let w: Whoami = serde_json::from_value(json).unwrap();
        assert_eq!(w.team_id, Some(53623));
        assert_eq!(w.team_name.as_deref(), Some("acme"));
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
        let json =
            serde_json::json!({ "team_id": 12345, "team_name": "Acme Corp", "team_url": "acme" });
        let w: Whoami = serde_json::from_value(json).unwrap();
        assert_eq!(w.team_url.as_deref(), Some("acme"));
    }

    #[test]
    fn team_subdomain_prefers_team_url() {
        let w = Whoami {
            team_id: None,
            team_name: Some("Acme Corp".to_string()),
            user_name: None,
            team_url: Some("acme".to_string()),
        };
        assert_eq!(team_subdomain(&w), Some("acme".to_string()));
    }

    #[test]
    fn team_subdomain_falls_back_to_team_name() {
        let w = Whoami {
            team_id: None,
            team_name: Some("acme".to_string()),
            user_name: None,
            team_url: None,
        };
        assert_eq!(team_subdomain(&w), Some("acme".to_string()));
    }

    #[test]
    fn team_subdomain_lowercases() {
        let w = Whoami {
            team_id: None,
            team_name: None,
            user_name: None,
            team_url: Some("ACME".to_string()),
        };
        assert_eq!(team_subdomain(&w), Some("acme".to_string()));
    }

    #[test]
    fn team_subdomain_rejects_invalid_label() {
        let w = Whoami {
            team_id: None,
            team_name: Some("Acme Corp".to_string()), // contains a space
            user_name: None,
            team_url: None,
        };
        assert_eq!(team_subdomain(&w), None);
    }

    #[test]
    fn team_subdomain_none_when_both_absent() {
        let w = Whoami {
            team_id: Some(1),
            team_name: None,
            user_name: None,
            team_url: None,
        };
        assert_eq!(team_subdomain(&w), None);
    }
}
