use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub severity: Option<String>,
    pub state: Option<String>,
    pub status: Option<String>,
    pub created_at: Option<String>,
    pub closed_at: Option<String>,
    pub is_muted: Option<bool>,
    pub assignments: Option<Vec<Assignment>>,
    pub meta_labels: Option<Vec<MetaLabel>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    pub assigned_to: Option<AssignedTo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignedTo {
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaLabel {
    pub key: Option<String>,
    pub value: Option<String>,
}

impl Incident {
    pub fn display_severity(&self) -> String {
        self.severity
            .as_deref()
            .map(|s| {
                s.strip_prefix("INCIDENT_SEVERITY_")
                    .unwrap_or(s)
                    .to_string()
            })
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn display_state(&self) -> String {
        self.state
            .as_deref()
            .or(self.status.as_deref())
            .map(|s| {
                s.strip_prefix("INCIDENT_STATUS_")
                    .or_else(|| s.strip_prefix("INCIDENT_STATE_"))
                    .unwrap_or(s)
                    .to_string()
            })
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn display_assignees(&self) -> String {
        self.assignments
            .as_ref()
            .map(|assignments| {
                assignments
                    .iter()
                    .filter_map(|a| a.assigned_to.as_ref().and_then(|at| at.user_id.as_deref()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "-".to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListIncidentsResponse {
    #[serde(default)]
    pub incidents: Vec<Incident>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetIncidentResponse {
    pub incident: Option<Incident>,
}

#[derive(Debug, Deserialize)]
pub struct AcknowledgeIncidentsResponse {}

#[derive(Debug, Deserialize)]
pub struct ResolveIncidentsResponse {}

#[derive(Debug, Deserialize)]
pub struct CloseIncidentsResponse {}

#[derive(Debug, Deserialize)]
pub struct AssignIncidentsResponse {}

#[derive(Debug, Deserialize)]
pub struct UnassignIncidentsResponse {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentEvent {
    pub id: Option<String>,
    pub incident_id: Option<String>,
    #[serde(rename = "type")]
    pub event_type: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListIncidentEventsResponse {
    #[serde(default)]
    pub events: Vec<IncidentEvent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListIncidentAggregationsResponse {
    #[serde(default)]
    pub aggregations: Vec<Value>,
}

// --- API ---

const INCIDENTS_BASE: &str = "/mgmt/openapi/5/incidents/incidents/v1";
const INCIDENT_EVENTS_BASE: &str = "/mgmt/openapi/5/incidents/events/v1";
const INCIDENT_AGGREGATIONS_BASE: &str = "/mgmt/openapi/5/incidents/aggregations/v1";

pub struct IncidentsApi<'a> {
    client: &'a CxClient,
}

impl<'a> IncidentsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// List incidents (POST with filter body).
    pub async fn list(&self, body: &Value) -> Result<ListIncidentsResponse> {
        self.client.post(INCIDENTS_BASE, body).await
    }

    /// Get a single incident by ID.
    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{INCIDENTS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    /// Acknowledge incidents by IDs.
    pub async fn acknowledge(
        &self,
        incident_ids: &[String],
    ) -> Result<AcknowledgeIncidentsResponse> {
        let path = format!("{INCIDENTS_BASE}/all/acknowledge");
        let body = serde_json::json!({ "incidentIds": incident_ids });
        self.client.post(&path, &body).await
    }

    /// Resolve incidents by IDs.
    pub async fn resolve(&self, incident_ids: &[String]) -> Result<ResolveIncidentsResponse> {
        let path = format!("{INCIDENTS_BASE}/all/resolve");
        let body = serde_json::json!({ "incidentIds": incident_ids });
        self.client.post(&path, &body).await
    }

    /// Close incidents by IDs.
    pub async fn close(&self, incident_ids: &[String]) -> Result<CloseIncidentsResponse> {
        let path = format!("{INCIDENTS_BASE}/all/closed");
        let body = serde_json::json!({ "incidentIds": incident_ids });
        self.client.post(&path, &body).await
    }

    /// Assign incidents to a user.
    pub async fn assign(
        &self,
        incident_ids: &[String],
        user_id: &str,
    ) -> Result<AssignIncidentsResponse> {
        let path = format!("{INCIDENTS_BASE}/all/by-user");
        let body = serde_json::json!({
            "incidentIds": incident_ids,
            "userId": user_id,
        });
        self.client.post(&path, &body).await
    }

    /// Unassign incidents.
    pub async fn unassign(&self, incident_ids: &[String]) -> Result<UnassignIncidentsResponse> {
        let path = format!("{INCIDENTS_BASE}/all/by-user");
        let body = serde_json::json!({ "incidentIds": incident_ids });
        self.client.delete_with_body(&path, &body).await
    }

    /// List incident events.
    pub async fn list_events(
        &self,
        incident_id: Option<&str>,
    ) -> Result<ListIncidentEventsResponse> {
        let mut params = Vec::new();
        if let Some(id) = incident_id {
            params.push(("incident_id", id));
        }
        self.client.get(INCIDENT_EVENTS_BASE, &params).await
    }

    /// Get incident aggregations.
    pub async fn aggregations(&self) -> Result<ListIncidentAggregationsResponse> {
        self.client.get(INCIDENT_AGGREGATIONS_BASE, &[]).await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_list_response() {
        let json = json!({
            "incidents": [
                {
                    "id": "inc-123",
                    "name": "High Error Rate",
                    "severity": "INCIDENT_SEVERITY_CRITICAL",
                    "state": "INCIDENT_STATE_TRIGGERED",
                    "createdAt": "2026-04-01T12:00:00Z"
                },
                {
                    "id": "inc-456",
                    "name": "CPU Spike",
                    "severity": "INCIDENT_SEVERITY_WARNING",
                    "state": "INCIDENT_STATE_ACKNOWLEDGED"
                }
            ]
        });

        let resp: ListIncidentsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.incidents.len(), 2);
        assert_eq!(resp.incidents[0].id.as_deref(), Some("inc-123"));
        assert_eq!(resp.incidents[0].display_severity(), "CRITICAL");
        assert_eq!(resp.incidents[0].display_state(), "TRIGGERED");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListIncidentsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.incidents.is_empty());
    }

    #[test]
    fn display_severity_strips_prefix() {
        let incident = Incident {
            id: None,
            name: None,
            description: None,
            severity: Some("INCIDENT_SEVERITY_WARNING".to_string()),
            state: None,
            status: None,
            created_at: None,
            closed_at: None,
            is_muted: None,
            assignments: None,
            meta_labels: None,
        };
        assert_eq!(incident.display_severity(), "WARNING");
    }

    #[test]
    fn display_state_strips_prefix() {
        let incident = Incident {
            id: None,
            name: None,
            description: None,
            severity: None,
            state: Some("INCIDENT_STATE_TRIGGERED".to_string()),
            status: None,
            created_at: None,
            closed_at: None,
            is_muted: None,
            assignments: None,
            meta_labels: None,
        };
        assert_eq!(incident.display_state(), "TRIGGERED");
    }

    #[test]
    fn display_state_falls_back_to_status() {
        let incident = Incident {
            id: None,
            name: None,
            description: None,
            severity: None,
            state: None,
            status: Some("INCIDENT_STATUS_RESOLVED".to_string()),
            created_at: None,
            closed_at: None,
            is_muted: None,
            assignments: None,
            meta_labels: None,
        };
        assert_eq!(incident.display_state(), "RESOLVED");
    }

    #[test]
    fn display_assignees_from_assignments() {
        let incident = Incident {
            id: None,
            name: None,
            description: None,
            severity: None,
            state: None,
            status: None,
            created_at: None,
            closed_at: None,
            is_muted: None,
            assignments: Some(vec![
                Assignment {
                    assigned_to: Some(AssignedTo {
                        user_id: Some("user-1".to_string()),
                    }),
                },
                Assignment {
                    assigned_to: Some(AssignedTo {
                        user_id: Some("user-2".to_string()),
                    }),
                },
            ]),
            meta_labels: None,
        };
        assert_eq!(incident.display_assignees(), "user-1, user-2");
    }

    #[test]
    fn display_missing_fields() {
        let incident = Incident {
            id: None,
            name: None,
            description: None,
            severity: None,
            state: None,
            status: None,
            created_at: None,
            closed_at: None,
            is_muted: None,
            assignments: None,
            meta_labels: None,
        };
        assert_eq!(incident.display_severity(), "-");
        assert_eq!(incident.display_state(), "-");
        assert_eq!(incident.display_assignees(), "-");
    }

    #[test]
    fn deserialize_events_response() {
        let json = json!({
            "events": [
                {
                    "id": "evt-1",
                    "incidentId": "inc-123",
                    "type": "TRIGGERED",
                    "createdAt": "2026-04-01T12:00:00Z"
                }
            ]
        });
        let resp: ListIncidentEventsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.events.len(), 1);
        assert_eq!(resp.events[0].id.as_deref(), Some("evt-1"));
    }
}
