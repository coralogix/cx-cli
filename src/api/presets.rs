use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: Option<String>,
    pub name: Option<String>,
    pub connector_type: Option<String>,
    pub is_default: Option<bool>,
    pub is_custom: Option<bool>,
}

impl Preset {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_connector_type(&self) -> String {
        self.connector_type
            .as_deref()
            .map(|s| s.strip_prefix("CONNECTOR_TYPE_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPresetsResponse {
    #[serde(default)]
    pub presets: Vec<Preset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPresetResponse {
    pub preset: Option<Preset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePresetResponse {
    pub preset: Option<Preset>,
}

#[derive(Debug, Deserialize)]
pub struct DeletePresetResponse {}

#[derive(Debug, Deserialize)]
pub struct SetDefaultPresetResponse {}

// --- API ---

const PRESETS_BASE: &str = "/mgmt/openapi/latest/notifications/notification-center/v1/presets";

pub struct PresetsApi<'a> {
    client: &'a CxClient,
}

impl<'a> PresetsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListPresetsResponse> {
        self.client.get(PRESETS_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{PRESETS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreatePresetResponse> {
        let path = format!("{PRESETS_BASE}/custom");
        self.client.post(&path, body).await
    }

    pub async fn replace(&self, body: &Value) -> Result<Value> {
        let path = format!("{PRESETS_BASE}/custom");
        self.client.put(&path, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeletePresetResponse> {
        let path = format!("{PRESETS_BASE}/custom/{id}");
        self.client.delete(&path).await
    }

    pub async fn set_default(&self, id: &str) -> Result<SetDefaultPresetResponse> {
        let path = format!("{PRESETS_BASE}/{id}/default");
        self.client.post(&path, &serde_json::json!({})).await
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
            "presets": [
                {
                    "id": "preset-001",
                    "name": "Default Slack",
                    "connectorType": "CONNECTOR_TYPE_SLACK",
                    "isDefault": true,
                    "isCustom": false
                },
                {
                    "id": "preset-002",
                    "name": "Custom PD",
                    "connectorType": "CONNECTOR_TYPE_PAGERDUTY",
                    "isDefault": false,
                    "isCustom": true
                }
            ]
        });

        let resp: ListPresetsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.presets.len(), 2);
        assert_eq!(resp.presets[0].display_name(), "Default Slack");
        assert_eq!(resp.presets[0].display_connector_type(), "SLACK");
        assert_eq!(resp.presets[0].is_default, Some(true));
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListPresetsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.presets.is_empty());
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeletePresetResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn display_missing_fields() {
        let preset = Preset {
            id: None,
            name: None,
            connector_type: None,
            is_default: None,
            is_custom: None,
        };
        assert_eq!(preset.display_name(), "-");
        assert_eq!(preset.display_connector_type(), "-");
    }
}
