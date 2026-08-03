use anyhow::anyhow;
use serde::Deserialize;

use crate::api_client::CxClient;

const WHOAMI_BASE: &str = "/identity/whoami";

/// Response from `GET /identity/whoami`. Identifies the team that the
/// current API key belongs to. Payload uses snake_case field names.
///
/// Only used to resolve a numeric team ID for building REST API paths (e.g.
/// `iam users`/`cases` team-scoped endpoints). Console-link resolution does
/// *not* use this endpoint - it relies solely on explicit config (see
/// `Profile::console_url` / `Profile::console_team_name`).
#[derive(Debug, Deserialize)]
pub struct Whoami {
    pub team_id: Option<i64>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_whoami() {
        let json = serde_json::json!({ "team_id": 53623, "team_name": "acme", "user_name": "alice@example.com" });
        let w: Whoami = serde_json::from_value(json).unwrap();
        assert_eq!(w.team_id, Some(53623));
    }

    #[test]
    fn deserialize_whoami_minimal() {
        let json = serde_json::json!({ "team_id": 12345 });
        let w: Whoami = serde_json::from_value(json).unwrap();
        assert_eq!(w.team_id, Some(12345));
    }
}
