use std::collections::HashMap;

use anyhow::anyhow;
use serde::Deserialize;
use serde_json::Value;

use crate::api_client::CxClient;
use crate::error::Result;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Case {
    pub id: Option<String>,
    pub readable_id: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub priority_details: Option<PriorityDetails>,
    pub category: Option<String>,
    pub assignee: Option<UserDetails>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
    pub acknowledge_time: Option<String>,
    pub resolution_details: Option<Value>,
    pub duration: Option<Value>,
    pub ai_summary: Option<String>,
    #[serde(default)]
    pub labels: Vec<KeyValue>,
    #[serde(default)]
    pub groupings: Vec<KeyValue>,
    #[serde(default)]
    pub impacted_entities: Vec<Value>,
    pub kpi_breaches: Option<Value>,
    pub case_indicators: Option<Value>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserDetails {
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PriorityDetails {
    pub system: Option<String>,
    pub r#override: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct KeyValue {
    pub key: Option<String>,
    pub value: Option<String>,
}

impl Case {
    /// Strip "CASE_STATUS_" prefix → "ACTIVE".
    pub fn display_status(&self) -> String {
        self.status
            .as_deref()
            .map(|s| s.strip_prefix("CASE_STATUS_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    /// Strip "CASE_PRIORITY_" prefix → "P1".
    pub fn display_priority(&self) -> String {
        self.priority
            .as_deref()
            .map(|s| s.strip_prefix("CASE_PRIORITY_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    /// Strip "CASE_CATEGORY_" prefix → "AVAILABILITY".
    pub fn display_category(&self) -> String {
        self.category
            .as_deref()
            .map(|s| s.strip_prefix("CASE_CATEGORY_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn display_assignee(&self) -> String {
        self.assignee
            .as_ref()
            .and_then(|a| a.user_id.clone())
            .unwrap_or_else(|| "-".to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEventsResponse {
    #[serde(default)]
    pub events: Vec<Value>,
}

/// One user row from the team users search endpoint
/// `GET /mgmt/openapi/5/aaa/teams/v2/{team_id}/search`. Only the fields we use
/// are modeled; the endpoint also returns firstName/lastName/status etc.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TeamUser {
    /// Opaque user ID (UUID). Matches a case's `assignee.userId`.
    pub user_id: Option<String>,
    /// The user's email address (the API field is named `username`).
    pub username: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchUsersResponse {
    #[serde(default)]
    pub users: Vec<TeamUser>,
    /// Offset token for the next page; absent once the last page is reached.
    pub next_page_token: Option<i64>,
}

/// Response from `GET /identity/whoami`, which identifies the team (and user)
/// the current API key belongs to. Fields are snake_case in the payload.
#[derive(Debug, Deserialize)]
pub struct Whoami {
    pub team_id: Option<i64>,
    pub team_name: Option<String>,
    pub user_name: Option<String>,
}

/// In-memory lookup over the team-members list, used to convert between
/// opaque user IDs (the API's currency) and email addresses (what humans
/// recognize). Built once per command invocation per profile.
#[derive(Debug, Default, Clone)]
pub struct TeammateDirectory {
    by_id: HashMap<String, String>,
    by_email: HashMap<String, String>,
}

impl TeammateDirectory {
    pub fn from_users(users: Vec<TeamUser>) -> Self {
        let mut by_id = HashMap::new();
        let mut by_email = HashMap::new();
        for u in users {
            if let (Some(id), Some(email)) = (u.user_id, u.username) {
                by_id.insert(id.clone(), email.clone());
                by_email.insert(email.to_lowercase(), id);
            }
        }
        Self { by_id, by_email }
    }

    pub fn email_for(&self, user_id: &str) -> Option<&str> {
        self.by_id.get(user_id).map(String::as_str)
    }

    pub fn user_id_for_email(&self, email: &str) -> Option<&str> {
        self.by_email.get(&email.to_lowercase()).map(String::as_str)
    }

    /// Resolve user input (which may be an email or a raw user ID) to a user ID.
    ///
    /// Inputs containing `@` are treated as emails and must be found in the
    /// directory or an error is returned. Other inputs are passed through
    /// unchanged on the assumption that they are already a user ID — the
    /// server will reject malformed values.
    pub fn resolve_to_user_id(&self, input: &str) -> anyhow::Result<String> {
        if input.contains('@') {
            self.user_id_for_email(input)
                .map(String::from)
                .ok_or_else(|| {
                    anyhow!(
                        "no team member found with email '{input}'. \
                         Run `cx iam users search` (or check `cx cases get <id>`) for the right address."
                    )
                })
        } else {
            Ok(input.to_string())
        }
    }
}

// --- API client ---

const CASES_BASE: &str = "/mgmt/openapi/5/cases/cases/v1";
const EVENTS_BASE: &str = "/mgmt/openapi/5/cases/events/v1";
const PRIORITY_BASE: &str = "/mgmt/openapi/5/cases/priority-override/v1";
const ASSIGNED_BASE: &str = "/mgmt/openapi/5/cases/assigned/v1";
const ACKNOWLEDGED_BASE: &str = "/mgmt/openapi/5/cases/acknowledged/v1";
const CLOSED_BASE: &str = "/mgmt/openapi/5/cases/closed/v1";
const RESOLVED_BASE: &str = "/mgmt/openapi/5/cases/resolved/v1";
const NOTIFICATION_DELIVERIES_BASE: &str = "/mgmt/openapi/5/cases/notifications/v1/deliveries";
const USERS_BASE: &str = "/mgmt/openapi/5/aaa/teams/v2";
const WHOAMI_BASE: &str = "/identity/whoami";
/// Page size for the paginated team-users search. The endpoint caps how many
/// rows it returns per call, so we walk pages until `nextPageToken` is absent.
const USERS_PAGE_SIZE: i64 = 300;

pub struct CasesApi<'a> {
    client: &'a CxClient,
}

impl<'a> CasesApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// Get a single case by ID.
    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{CASES_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    /// Update a case (partial patch). The endpoint takes the updatable fields
    /// directly as the request body — it has no `patch` wrapper field.
    pub async fn update(&self, id: &str, patch: &Value) -> Result<Value> {
        let path = format!("{CASES_BASE}/{id}");
        self.client.put(&path, patch).await
    }

    /// Assign a case to a user.
    pub async fn assign(&self, id: &str, user_id: &str) -> Result<Value> {
        let path = format!("{ASSIGNED_BASE}/{id}");
        let body = serde_json::json!({ "assignee": { "userId": user_id } });
        self.client.post(&path, &body).await
    }

    /// Remove the assignee from a case.
    pub async fn unassign(&self, id: &str) -> Result<Value> {
        let path = format!("{ASSIGNED_BASE}/{id}");
        self.client.delete(&path).await
    }

    /// Acknowledge a case.
    pub async fn acknowledge(&self, id: &str) -> Result<Value> {
        let path = format!("{ACKNOWLEDGED_BASE}/{id}");
        let body = serde_json::json!({});
        self.client.put(&path, &body).await
    }

    /// Remove the acknowledgment from a case.
    pub async fn unacknowledge(&self, id: &str) -> Result<Value> {
        let path = format!("{ACKNOWLEDGED_BASE}/{id}");
        self.client.delete(&path).await
    }

    /// Close a case.
    pub async fn close(&self, id: &str) -> Result<Value> {
        let path = format!("{CLOSED_BASE}/{id}");
        let body = serde_json::json!({});
        self.client.post(&path, &body).await
    }

    /// Resolve a case with an optional reason.
    pub async fn resolve(&self, id: &str, reason: Option<&str>) -> Result<Value> {
        let path = format!("{RESOLVED_BASE}/{id}");
        let mut body = serde_json::Map::new();
        if let Some(r) = reason {
            body.insert("reason".to_string(), Value::String(r.to_string()));
        }
        self.client.put(&path, &Value::Object(body)).await
    }

    /// Override a case's priority.
    pub async fn set_priority(&self, id: &str, priority: &str) -> Result<Value> {
        let path = format!("{PRIORITY_BASE}/{id}");
        let body = serde_json::json!({ "priority": priority });
        self.client.put(&path, &body).await
    }

    /// Clear a case's priority override.
    pub async fn clear_priority(&self, id: &str) -> Result<Value> {
        let path = format!("{PRIORITY_BASE}/{id}");
        self.client.delete(&path).await
    }

    /// List events for a case.
    pub async fn list_events(&self, case_id: &str) -> Result<ListEventsResponse> {
        let path = format!("{CASES_BASE}/{case_id}/events");
        self.client.get(&path, &[]).await
    }

    /// Add a comment to a case. The body field is `text`; the server stores it
    /// as a comment event (`eventData.comment.unsafeText`) and returns the
    /// created event.
    pub async fn create_comment(&self, case_id: &str, text: &str) -> Result<Value> {
        let path = format!("{CASES_BASE}/{case_id}/comments");
        let body = serde_json::json!({ "text": text });
        self.client.post(&path, &body).await
    }

    /// Get a single case event by event ID.
    pub async fn get_event(&self, event_id: &str) -> Result<Value> {
        let path = format!("{EVENTS_BASE}/{event_id}");
        self.client.get(&path, &[]).await
    }

    /// List notification deliveries for one or more cases. The endpoint accepts
    /// both readable IDs (e.g. "CASE-764019") and UUIDs in `caseIds`, and keys
    /// the `deliveriesByCase` response under whatever form was passed.
    pub async fn list_notification_deliveries(&self, case_ids: &[String]) -> Result<Value> {
        let body = serde_json::json!({ "caseIds": case_ids });
        self.client.post(NOTIFICATION_DELIVERIES_BASE, &body).await
    }

    /// Resolve the team ID for the current API key. The team-users search
    /// endpoint embeds the team ID in its path, so we ask `/identity/whoami`,
    /// which every key can read and which returns the caller's team.
    async fn resolve_team_id(&self) -> anyhow::Result<String> {
        let whoami: Whoami = self
            .client
            .get(WHOAMI_BASE, &[])
            .await
            .map_err(|e| anyhow!("failed to resolve team ID via /identity/whoami: {e:#}"))?;
        whoami
            .team_id
            .map(|id| id.to_string())
            .ok_or_else(|| anyhow!("/identity/whoami returned no team_id"))
    }

    /// List all team members by walking the paginated team-users search
    /// endpoint (`GET /mgmt/openapi/5/aaa/teams/v2/{team_id}/search`) in pages
    /// of [`USERS_PAGE_SIZE`], following `nextPageToken` until it is absent.
    pub async fn list_teammates(&self) -> Result<Vec<TeamUser>> {
        let team_id = self
            .resolve_team_id()
            .await
            .map_err(|e| crate::error::CxError::Api {
                status: 0,
                message: e.to_string(),
            })?;
        let path = format!("{USERS_BASE}/{team_id}/search");
        let page_size = USERS_PAGE_SIZE.to_string();

        let mut users = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut params: Vec<(&str, &str)> = vec![("pageSize", page_size.as_str())];
            if let Some(ref token) = page_token {
                params.push(("pageToken", token.as_str()));
            }
            let resp: SearchUsersResponse = self.client.get(&path, &params).await?;
            let page_len = resp.users.len();
            users.extend(resp.users);
            // Stop once the server reports no further page, or returns an empty
            // page (defensive: avoids looping if a token is ever echoed back).
            match resp.next_page_token {
                Some(token) if page_len > 0 => page_token = Some(token.to_string()),
                _ => break,
            }
        }
        Ok(users)
    }

    /// Convenience: fetch teammates and build a [`TeammateDirectory`].
    /// Errors are surfaced; callers that want best-effort behavior should
    /// `.unwrap_or_default()`.
    pub async fn teammate_directory(&self) -> Result<TeammateDirectory> {
        let users = self.list_teammates().await?;
        Ok(TeammateDirectory::from_users(users))
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_case() {
        let unassigned: Case = serde_json::from_value(json!({
            "id": "3f166e9f-3c88-4af2-b52e-138f339dab3e",
            "readableId": "CASE-123",
            "title": "Database outage investigation",
            "status": "CASE_STATUS_ACTIVE",
            "priority": "CASE_PRIORITY_P2",
            "category": "CASE_CATEGORY_AVAILABILITY",
            "createTime": "2025-09-22T10:30:00Z"
        }))
        .unwrap();
        assert_eq!(unassigned.display_status(), "ACTIVE");
        assert_eq!(unassigned.display_priority(), "P2");
        assert_eq!(unassigned.display_category(), "AVAILABILITY");
        assert_eq!(unassigned.display_assignee(), "-");

        let assigned: Case = serde_json::from_value(json!({
            "id": "a4c7d2e8-5f99-4b3a-c53f-239f440dbe4f",
            "title": "Security incident",
            "status": "CASE_STATUS_ACKNOWLEDGED",
            "priority": "CASE_PRIORITY_P1",
            "category": "CASE_CATEGORY_SECURITY",
            "createTime": "2025-09-23T09:00:00Z",
            "assignee": { "userId": "user-1" }
        }))
        .unwrap();
        assert_eq!(assigned.display_status(), "ACKNOWLEDGED");
        assert_eq!(assigned.display_assignee(), "user-1");
    }

    #[test]
    fn display_missing_fields() {
        let case = Case {
            id: None,
            readable_id: None,
            title: None,
            status: None,
            priority: None,
            priority_details: None,
            category: None,
            assignee: None,
            create_time: None,
            update_time: None,
            acknowledge_time: None,
            resolution_details: None,
            duration: None,
            ai_summary: None,
            labels: vec![],
            groupings: vec![],
            impacted_entities: vec![],
            kpi_breaches: None,
            case_indicators: None,
        };
        assert_eq!(case.display_status(), "-");
        assert_eq!(case.display_priority(), "-");
        assert_eq!(case.display_category(), "-");
        assert_eq!(case.display_assignee(), "-");
    }

    #[test]
    fn deserialize_priority_details() {
        let body = json!({
            "system": "CASE_PRIORITY_P2",
            "override": "CASE_PRIORITY_P1"
        });
        let pd: PriorityDetails = serde_json::from_value(body).unwrap();
        assert_eq!(pd.system.as_deref(), Some("CASE_PRIORITY_P2"));
        assert_eq!(pd.r#override.as_deref(), Some("CASE_PRIORITY_P1"));
    }

    #[test]
    fn deserialize_events_response() {
        let body = json!({
            "events": [
                {
                    "id": "f56645c5-9cd4-4b9f-961f-4f852d8835a0",
                    "type": "EVENT_TYPE_COMMENT",
                    "createTime": "2025-09-22T11:00:00Z"
                },
                {
                    "id": "0a08c310-ae44-6009-28ec-b6b7da50a99c",
                    "type": "EVENT_TYPE_STATUS_CHANGE"
                }
            ]
        });
        let resp: ListEventsResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.events.len(), 2);
        assert_eq!(
            resp.events[0]["id"].as_str(),
            Some("f56645c5-9cd4-4b9f-961f-4f852d8835a0")
        );
    }

    #[test]
    fn deserialize_events_response_empty() {
        let resp: ListEventsResponse = serde_json::from_value(json!({})).unwrap();
        assert!(resp.events.is_empty());
    }

    #[test]
    fn deserialize_search_users_response() {
        let body = json!({
            "users": [
                {
                    "userId": "281da03a-0241-447e-aca0-f90083b8198a",
                    "userAccountId": 44584,
                    "status": "USER_STATUS_ACTIVE",
                    "username": "Alessandro.Massa@coralogix.com",
                    "firstName": "Alessandro",
                    "lastName": "Massa"
                },
                {
                    "userId": "60291e28-3bec-4bb4-b7e5-3deabcdc80d2",
                    "username": "ilia.shurygin@coralogix.com"
                }
            ],
            "nextPageToken": 2,
            "totalCount": 2
        });
        let resp: SearchUsersResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.users.len(), 2);
        assert_eq!(
            resp.users[0].username.as_deref(),
            Some("Alessandro.Massa@coralogix.com")
        );
        assert_eq!(resp.next_page_token, Some(2));
    }

    #[test]
    fn deserialize_search_users_last_page() {
        // The last page omits `nextPageToken`.
        let body = json!({ "users": [], "totalCount": 0 });
        let resp: SearchUsersResponse = serde_json::from_value(body).unwrap();
        assert!(resp.users.is_empty());
        assert_eq!(resp.next_page_token, None);
    }

    #[test]
    fn directory_round_trips_id_and_email() {
        let dir = TeammateDirectory::from_users(vec![
            TeamUser {
                user_id: Some("uid-1".into()),
                username: Some("alice@example.com".into()),
            },
            TeamUser {
                user_id: Some("uid-2".into()),
                username: Some("Bob@Example.com".into()),
            },
        ]);

        assert_eq!(dir.email_for("uid-1"), Some("alice@example.com"));
        assert_eq!(dir.email_for("uid-2"), Some("Bob@Example.com"));
        assert_eq!(dir.email_for("nope"), None);

        // Lookup is case-insensitive on the email side.
        assert_eq!(dir.user_id_for_email("alice@example.com"), Some("uid-1"));
        assert_eq!(dir.user_id_for_email("ALICE@example.com"), Some("uid-1"));
        assert_eq!(dir.user_id_for_email("bob@example.com"), Some("uid-2"));
    }

    #[test]
    fn directory_resolve_email_to_id() {
        let dir = TeammateDirectory::from_users(vec![TeamUser {
            user_id: Some("uid-1".into()),
            username: Some("alice@example.com".into()),
        }]);

        assert_eq!(
            dir.resolve_to_user_id("alice@example.com").unwrap(),
            "uid-1"
        );
        // Non-email input passes through unchanged.
        assert_eq!(dir.resolve_to_user_id("uid-1").unwrap(), "uid-1");
        assert_eq!(dir.resolve_to_user_id("some-uuid").unwrap(), "some-uuid");
        // Unknown email errors out.
        let err = dir.resolve_to_user_id("ghost@example.com").unwrap_err();
        assert!(err.to_string().contains("no team member found"));
    }

    #[test]
    fn deserialize_labels_and_groupings() {
        let case: Case = serde_json::from_value(json!({
            "title": "x",
            "labels": [{ "key": "team", "value": "backend" }],
            "groupings": [{ "key": "service", "value": "payments" }]
        }))
        .unwrap();
        assert_eq!(case.labels.len(), 1);
        assert_eq!(case.labels[0].key.as_deref(), Some("team"));
        assert_eq!(case.groupings[0].value.as_deref(), Some("payments"));
    }
}
