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
    /// The team's subdomain label, when the API returns one. Preferred over
    /// `team_name` for building console URLs when present and hostname-safe,
    /// but many real teams' `/identity/whoami` responses omit it entirely
    /// (confirmed against a live team during FORGE-586 review) - `team_name`
    /// is reliably present and, sanitized, is a good-enough guess at the real
    /// subdomain for the common case. See [`team_subdomain`].
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

/// Extract the hostname-safe team subdomain from a `Whoami` payload, used to
/// build "View in Coralogix" console links by default when a profile hasn't
/// set an explicit `console_team_name` override (see
/// `docs/configuration.md#console-links`).
///
/// Prefers `team_url` - only labels made up of lowercase ASCII letters,
/// digits, and hyphens are accepted from it; anything else (whitespace,
/// unicode, empty string) cannot be a valid hostname label, so it's treated
/// as absent rather than used to build a broken console link.
///
/// If `team_url` is absent or unusable, falls back to a *sanitized* guess
/// derived from `team_name` via [`sanitize_team_name_label`]. This fallback
/// runs unconditionally (it is the default behavior, not an opt-in): a real
/// team's `/identity/whoami` response was confirmed during review to omit
/// `team_url` while still returning a perfectly usable `team_name`, so
/// requiring `team_url` made the automatic path fail far more often than
/// necessary. `team_name` is a display name, not a URL label (e.g. a team
/// named "acmeprod" could have the real subdomain "acme-prod"), so this is
/// still only a best-effort guess - a profile can pin an exact
/// `console_team_name` (skips this lookup entirely) or a full `console_url`
/// override when the guess is wrong.
fn team_subdomain(whoami: &Whoami) -> Option<String> {
    if let Some(candidate) = whoami.team_url.as_deref() {
        let lower = candidate.to_lowercase();
        if !lower.is_empty()
            && lower
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Some(lower);
        }
    }
    whoami
        .team_name
        .as_deref()
        .and_then(sanitize_team_name_label)
}

/// Best-effort sanitize a team display name (e.g. `"Acme Corp"`) into a
/// hostname-safe label (e.g. `"acme-corp"`).
///
/// Lowercases the input, collapses every run of characters that aren't
/// ASCII letters/digits into a single hyphen, and trims leading/trailing
/// hyphens. Returns `None` if nothing hostname-safe remains (e.g. an empty
/// or entirely-punctuation name).
///
/// This is a *guess*, not a lookup - it cannot know that a team's real
/// subdomain diverges from a simple transliteration of its display name
/// (see [`team_subdomain`]'s doc comment).
fn sanitize_team_name_label(name: &str) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    let mut last_was_hyphen = true; // avoid a leading hyphen
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            out.push('-');
            last_was_hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Resolve the team subdomain used to build "View in Coralogix" console
/// links, e.g. the `acme` in `https://acme.app.eu2.coralogix.com`, via
/// `GET /identity/whoami`. This is the default automatic resolution path,
/// used whenever a profile hasn't set an explicit `console_team_name`
/// override (see `docs/configuration.md#console-links`).
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
    fn team_subdomain_falls_back_to_team_name_by_default_when_team_url_absent() {
        // Confirmed against a live team during FORGE-586 review: team_url is
        // absent for some real teams even though team_name is present and
        // usable, so the fallback must run unconditionally, not only when a
        // profile opts in.
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
    fn team_subdomain_falls_back_to_team_name_when_team_url_invalid() {
        // An invalid team_url (e.g. containing a space) must be treated as
        // unusable, falling through to the team_name path rather than
        // short-circuiting to None.
        let w = Whoami {
            team_id: None,
            team_name: Some("Acme Corp".to_string()),
            user_name: None,
            team_url: Some("Acme Corp".to_string()), // invalid: contains a space
        };
        assert_eq!(team_subdomain(&w), Some("acme-corp".to_string()));
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

    #[test]
    fn team_subdomain_prefers_valid_team_url_over_team_name() {
        let w = Whoami {
            team_id: None,
            team_name: Some("Totally Different Name".to_string()),
            user_name: None,
            team_url: Some("acme".to_string()),
        };
        assert_eq!(team_subdomain(&w), Some("acme".to_string()));
    }

    #[test]
    fn sanitize_team_name_label_collapses_spaces_and_punctuation() {
        assert_eq!(
            sanitize_team_name_label("Acme Corp"),
            Some("acme-corp".to_string())
        );
        assert_eq!(sanitize_team_name_label("C4C!!"), Some("c4c".to_string()));
        assert_eq!(
            sanitize_team_name_label("  Weird__Name--Here  "),
            Some("weird-name-here".to_string())
        );
    }

    #[test]
    fn sanitize_team_name_label_none_for_empty_or_all_punctuation() {
        assert_eq!(sanitize_team_name_label(""), None);
        assert_eq!(sanitize_team_name_label("   "), None);
        assert_eq!(sanitize_team_name_label("!!!"), None);
    }
}
