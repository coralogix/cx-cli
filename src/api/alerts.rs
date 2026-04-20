use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertDef {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub priority: Option<String>,
    #[serde(rename = "type")]
    pub alert_type: Option<String>,
    pub status: Option<String>,
    pub created_time: Option<String>,
    pub updated_time: Option<String>,
    pub last_triggered_time: Option<String>,
    pub alert_def_properties: Option<Value>,
}

impl AlertDef {
    /// Strip "ALERT_DEF_PRIORITY_" prefix → "P3"
    pub fn display_priority(&self) -> String {
        self.priority
            .as_deref()
            .or_else(|| {
                self.alert_def_properties
                    .as_ref()
                    .and_then(|p| p.get("priority"))
                    .and_then(|v| v.as_str())
            })
            .map(|s| {
                s.strip_prefix("ALERT_DEF_PRIORITY_")
                    .unwrap_or(s)
                    .to_string()
            })
            .unwrap_or_else(|| "-".to_string())
    }

    /// Strip "ALERT_DEF_TYPE_" prefix and convert to human name → "Logs Threshold"
    pub fn display_type(&self) -> String {
        self.alert_type
            .as_deref()
            .or_else(|| {
                self.alert_def_properties
                    .as_ref()
                    .and_then(|p| p.get("type"))
                    .and_then(|v| v.as_str())
            })
            .map(|s| {
                let stripped = s.strip_prefix("ALERT_DEF_TYPE_").unwrap_or(s);
                stripped
                    .split('_')
                    .map(|w| {
                        let mut c = w.chars();
                        match c.next() {
                            None => String::new(),
                            Some(first) => {
                                first.to_uppercase().to_string() + &c.as_str().to_lowercase()
                            }
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| "-".to_string())
    }

    /// Get name, falling back to alert_def_properties.name
    pub fn display_name(&self) -> String {
        self.name
            .as_deref()
            .or_else(|| {
                self.alert_def_properties
                    .as_ref()
                    .and_then(|p| p.get("name"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("-")
            .to_string()
    }

    /// Get description, falling back to alert_def_properties.description
    pub fn display_description(&self) -> String {
        self.description
            .as_deref()
            .or_else(|| {
                self.alert_def_properties
                    .as_ref()
                    .and_then(|p| p.get("description"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("")
            .to_string()
    }

    /// Get enabled, falling back to alert_def_properties.enabled
    pub fn display_enabled(&self) -> Option<bool> {
        self.enabled.or_else(|| {
            self.alert_def_properties
                .as_ref()
                .and_then(|p| p.get("enabled"))
                .and_then(|v| v.as_bool())
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAlertsResponse {
    #[serde(default)]
    pub alert_defs: Vec<AlertDef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAlertResponse {
    pub alert_def: Option<AlertDef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAlertResponse {
    pub alert_def: Option<AlertDef>,
}

#[derive(Debug, Deserialize)]
pub struct SetActiveResponse {}

// --- API ---

const ALERTS_BASE: &str = "/mgmt/openapi/latest/alerts/alerts-general/v3";

pub struct AlertsApi<'a> {
    client: &'a CxClient,
}

impl<'a> AlertsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// List all alert definitions.
    pub async fn list(&self) -> Result<ListAlertsResponse> {
        self.client.get(ALERTS_BASE, &[]).await
    }

    /// Get a single alert definition by alert def ID (returns raw JSON — preserves the full API response).
    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{ALERTS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    /// Get a single alert definition by alert version ID.
    pub async fn get_by_version_id(&self, version_id: &str) -> Result<Value> {
        let path = format!("{ALERTS_BASE}/alert-version-id/{version_id}");
        self.client.get(&path, &[]).await
    }

    /// Create an alert definition from a JSON body.
    pub async fn create(&self, body: &Value) -> Result<CreateAlertResponse> {
        self.client.post(ALERTS_BASE, body).await
    }

    /// Enable or disable an alert definition.
    pub async fn set_active(&self, id: &str, active: bool) -> Result<SetActiveResponse> {
        let path = format!("{ALERTS_BASE}/{id}:setActive");
        let val = if active { "true" } else { "false" };
        self.client.post_empty(&path, &[("active", val)]).await
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
            "alertDefs": [
                {
                    "id": "abc-123",
                    "name": "High Error Rate",
                    "enabled": true,
                    "priority": "ALERT_DEF_PRIORITY_P2",
                    "type": "ALERT_DEF_TYPE_LOGS_THRESHOLD",
                    "status": "OK",
                    "updatedTime": "2024-06-01T12:00:00Z"
                },
                {
                    "id": "def-456",
                    "name": "CPU Alert",
                    "enabled": false,
                    "priority": "ALERT_DEF_PRIORITY_P1",
                    "type": "ALERT_DEF_TYPE_METRIC_THRESHOLD",
                    "status": "ALERTING"
                }
            ]
        });

        let resp: ListAlertsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.alert_defs.len(), 2);
        assert_eq!(resp.alert_defs[0].id.as_deref(), Some("abc-123"));
        assert_eq!(resp.alert_defs[0].name.as_deref(), Some("High Error Rate"));
        assert_eq!(resp.alert_defs[0].enabled, Some(true));
        assert_eq!(resp.alert_defs[1].status.as_deref(), Some("ALERTING"));
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListAlertsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.alert_defs.is_empty());
    }

    #[test]
    fn display_priority_strips_prefix() {
        let alert = AlertDef {
            id: None,
            name: None,
            description: None,
            enabled: None,
            priority: Some("ALERT_DEF_PRIORITY_P3".to_string()),
            alert_type: None,
            status: None,
            created_time: None,
            updated_time: None,
            last_triggered_time: None,
            alert_def_properties: None,
        };
        assert_eq!(alert.display_priority(), "P3");
    }

    #[test]
    fn display_type_strips_prefix_and_titlecases() {
        let alert = AlertDef {
            id: None,
            name: None,
            description: None,
            enabled: None,
            priority: None,
            alert_type: Some("ALERT_DEF_TYPE_LOGS_THRESHOLD".to_string()),
            status: None,
            created_time: None,
            updated_time: None,
            last_triggered_time: None,
            alert_def_properties: None,
        };
        assert_eq!(alert.display_type(), "Logs Threshold");
    }

    #[test]
    fn display_type_metric_threshold() {
        let alert = AlertDef {
            id: None,
            name: None,
            description: None,
            enabled: None,
            priority: None,
            alert_type: Some("ALERT_DEF_TYPE_METRIC_THRESHOLD".to_string()),
            status: None,
            created_time: None,
            updated_time: None,
            last_triggered_time: None,
            alert_def_properties: None,
        };
        assert_eq!(alert.display_type(), "Metric Threshold");
    }

    #[test]
    fn display_fallback_to_properties() {
        let alert = AlertDef {
            id: None,
            name: None,
            description: None,
            enabled: None,
            priority: None,
            alert_type: None,
            status: None,
            created_time: None,
            updated_time: None,
            last_triggered_time: None,
            alert_def_properties: Some(json!({
                "name": "From Properties",
                "priority": "ALERT_DEF_PRIORITY_P5",
                "type": "ALERT_DEF_TYPE_LOGS_IMMEDIATE",
                "enabled": true
            })),
        };
        assert_eq!(alert.display_name(), "From Properties");
        assert_eq!(alert.display_priority(), "P5");
        assert_eq!(alert.display_type(), "Logs Immediate");
        assert_eq!(alert.display_enabled(), Some(true));
    }

    #[test]
    fn display_missing_fields() {
        let alert = AlertDef {
            id: None,
            name: None,
            description: None,
            enabled: None,
            priority: None,
            alert_type: None,
            status: None,
            created_time: None,
            updated_time: None,
            last_triggered_time: None,
            alert_def_properties: None,
        };
        assert_eq!(alert.display_name(), "-");
        assert_eq!(alert.display_priority(), "-");
        assert_eq!(alert.display_type(), "-");
        assert_eq!(alert.display_enabled(), None);
    }

    #[test]
    fn deserialize_get_response() {
        let json = json!({
            "alertDef": {
                "id": "abc-123",
                "name": "Test Alert"
            }
        });
        let resp: GetAlertResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.alert_def.unwrap().id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn deserialize_get_by_version_id_response() {
        // GetAlertDefByVersionIdResponse has the same shape as GetAlertResponse
        let json = json!({
            "alertDef": {
                "id": "abc-123",
                "name": "Versioned Alert"
            }
        });
        let resp: GetAlertResponse = serde_json::from_value(json).unwrap();
        let def = resp.alert_def.unwrap();
        assert_eq!(def.id.as_deref(), Some("abc-123"));
        assert_eq!(def.name.as_deref(), Some("Versioned Alert"));
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "alertDef": {
                "id": "new-id",
                "name": "Created Alert"
            }
        });
        let resp: CreateAlertResponse = serde_json::from_value(json).unwrap();
        let def = resp.alert_def.unwrap();
        assert_eq!(def.id.as_deref(), Some("new-id"));
        assert_eq!(def.name.as_deref(), Some("Created Alert"));
    }

    #[test]
    fn deserialize_set_active_response() {
        let json = json!({});
        let _resp: SetActiveResponse = serde_json::from_value(json).unwrap();
    }
}
