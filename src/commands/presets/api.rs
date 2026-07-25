use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;
use crate::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    pub connector_type: Option<String>,
    pub is_default: Option<bool>,
    pub is_custom: Option<bool>,
    pub preset_type: Option<String>,
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

    pub fn display_is_custom(&self) -> Option<bool> {
        self.is_custom.or_else(|| {
            self.preset_type
                .as_deref()
                .map(|preset_type| preset_type == "CUSTOM")
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPresetsResponse {
    #[serde(default, alias = "presetSummaries")]
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

const PRESETS_BASE: &str = "/mgmt/openapi/5/notifications/notification-center/v1/presets";

pub struct PresetsApi<'a> {
    client: &'a CxClient,
}

impl<'a> PresetsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListPresetsResponse> {
        let path = format!("{PRESETS_BASE}:summariesList");
        self.client.get(&path, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{PRESETS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreatePresetResponse> {
        let path = format!("{PRESETS_BASE}:createCustom");
        self.client.post(&path, body).await
    }

    pub async fn replace(&self, body: &Value) -> Result<Value> {
        let path = format!("{PRESETS_BASE}:replaceCustom");
        self.client.put(&path, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeletePresetResponse> {
        let path = format!("{PRESETS_BASE}/custom/{id}");
        self.client.delete(&path).await
    }

    pub async fn set_default(&self, id: &str) -> Result<SetDefaultPresetResponse> {
        let path = format!("{PRESETS_BASE}/{id}/default/apply");
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
            "presetSummaries": [
                {
                    "id": "preset-001",
                    "name": "Default Slack",
                    "connectorType": "SLACK",
                    "presetType": "SYSTEM"
                },
                {
                    "id": "preset-002",
                    "name": "Custom PD",
                    "connectorType": "PAGERDUTY",
                    "presetType": "CUSTOM"
                }
            ]
        });

        let resp: ListPresetsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.presets.len(), 2);
        assert_eq!(resp.presets[0].display_name(), "Default Slack");
        assert_eq!(resp.presets[0].display_connector_type(), "SLACK");
        assert_eq!(resp.presets[0].display_is_custom(), Some(false));
        assert_eq!(resp.presets[1].display_is_custom(), Some(true));
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
            preset_type: None,
        };
        assert_eq!(preset.display_name(), "-");
        assert_eq!(preset.display_connector_type(), "-");
    }
}
