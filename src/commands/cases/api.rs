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

/// One row from `GET /api/v1/user/team/teammates`. Only the fields we use are
/// modeled; the endpoint also returns groupIds, accountId, etc. that we ignore.
#[derive(Debug, Deserialize, Clone)]
pub struct Teammate {
    pub id: Option<String>,
    /// The team-member's email address (the API field is misnamed `username`).
    pub username: Option<String>,
    #[serde(rename = "firstName")]
    pub first_name: Option<String>,
    #[serde(rename = "lastName")]
    pub last_name: Option<String>,
    #[serde(rename = "isActive")]
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ListTeammatesResponse {
    #[serde(default)]
    pub data: Vec<Teammate>,
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
    pub fn from_response(resp: ListTeammatesResponse) -> Self {
        let mut by_id = HashMap::new();
        let mut by_email = HashMap::new();
        for t in resp.data {
            if let (Some(id), Some(email)) = (t.id, t.username) {
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
const TEAMMATES_BASE: &str = "/api/v1/user/team/teammates";

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

    /// Update a case (partial patch).
    pub async fn update(&self, id: &str, patch: &Value) -> Result<Value> {
        let path = format!("{CASES_BASE}/{id}");
        let body = serde_json::json!({ "patch": patch });
        self.client.put(&path, &body).await
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

    /// Get a single case event by event ID.
    pub async fn get_event(&self, event_id: &str) -> Result<Value> {
        let path = format!("{EVENTS_BASE}/{event_id}");
        self.client.get(&path, &[]).await
    }

    /// List notification deliveries for one or more cases.
    pub async fn list_notification_deliveries(&self, case_ids: &[String]) -> Result<Value> {
        let body = serde_json::json!({ "caseIds": case_ids });
        self.client.post(NOTIFICATION_DELIVERIES_BASE, &body).await
    }

    /// List team members (used to map user IDs <-> email addresses).
    pub async fn list_teammates(&self) -> Result<ListTeammatesResponse> {
        self.client.get(TEAMMATES_BASE, &[]).await
    }

    /// Convenience: fetch teammates and build a [`TeammateDirectory`].
    /// Errors are surfaced; callers that want best-effort behavior should
    /// `.unwrap_or_default()`.
    pub async fn teammate_directory(&self) -> Result<TeammateDirectory> {
        let resp = self.list_teammates().await?;
        Ok(TeammateDirectory::from_response(resp))
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
    fn deserialize_teammates_response() {
        let body = json!({
            "data": [
                {
                    "id": "281da03a-0241-447e-aca0-f90083b8198a",
                    "isActive": true,
                    "username": "Alessandro.Massa@coralogix.com",
                    "firstName": "Alessandro",
                    "lastName": "Massa"
                },
                {
                    "id": "60291e28-3bec-4bb4-b7e5-3deabcdc80d2",
                    "username": "ilia.shurygin@coralogix.com"
                }
            ]
        });
        let resp: ListTeammatesResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert_eq!(
            resp.data[0].username.as_deref(),
            Some("Alessandro.Massa@coralogix.com")
        );
    }

    #[test]
    fn directory_round_trips_id_and_email() {
        let body = json!({
            "data": [
                { "id": "uid-1", "username": "alice@example.com" },
                { "id": "uid-2", "username": "Bob@Example.com" }
            ]
        });
        let resp: ListTeammatesResponse = serde_json::from_value(body).unwrap();
        let dir = TeammateDirectory::from_response(resp);

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
        let body = json!({
            "data": [{ "id": "uid-1", "username": "alice@example.com" }]
        });
        let dir = TeammateDirectory::from_response(serde_json::from_value(body).unwrap());

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
