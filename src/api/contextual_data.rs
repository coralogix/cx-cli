use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;
use super::serde_helpers::string_or_number;

// --- Response types ---

/// The inner integration definition returned by the API.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextualDataIntegrationDef {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub integration_type: Option<Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
}

/// Wrapper returned in the list response: contains the integration definition
/// plus metadata like counts and deprecation flags.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationWithCounts {
    pub integration: Option<ContextualDataIntegrationDef>,
    pub amount_integrations: Option<i64>,
    #[serde(default)]
    pub is_deprecated: bool,
    #[serde(default)]
    pub is_new: bool,
    #[serde(default)]
    pub upgrade_available: bool,
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Flattened view used by the command layer for display and JSON output.
pub struct ContextualDataIntegration {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_deprecated: bool,
    pub amount_integrations: Option<i64>,
}

impl ContextualDataIntegration {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_description(&self) -> &str {
        self.description.as_deref().unwrap_or("-")
    }
}

impl From<IntegrationWithCounts> for ContextualDataIntegration {
    fn from(w: IntegrationWithCounts) -> Self {
        let (id, name, description) = match w.integration {
            Some(def) => (def.id, def.name, def.description),
            None => (None, None, None),
        };
        Self {
            id,
            name,
            description,
            is_deprecated: w.is_deprecated,
            amount_integrations: w.amount_integrations,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListContextualDataResponse {
    #[serde(default)]
    pub integrations: Vec<IntegrationWithCounts>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveContextualDataResponse {
    pub integration_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteContextualDataResponse {}

// --- API ---

const CONTEXTUAL_DATA_BASE: &str = "/mgmt/openapi/5/integrations/contextual-data/v1";

pub struct ContextualDataApi<'a> {
    client: &'a CxClient,
}

impl<'a> ContextualDataApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListContextualDataResponse> {
        self.client.get(CONTEXTUAL_DATA_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{CONTEXTUAL_DATA_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn save(&self, body: &Value) -> Result<SaveContextualDataResponse> {
        self.client.post(CONTEXTUAL_DATA_BASE, body).await
    }

    pub async fn update(&self, id: &str, body: &Value) -> Result<Value> {
        let path = format!("{CONTEXTUAL_DATA_BASE}/{id}");
        self.client.put(&path, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteContextualDataResponse> {
        let path = format!("{CONTEXTUAL_DATA_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn get_definition(&self, id: &str) -> Result<Value> {
        let path = format!("{CONTEXTUAL_DATA_BASE}/{id}/definition");
        self.client.get(&path, &[]).await
    }

    pub async fn test(&self, id: &str) -> Result<Value> {
        let path = format!("{CONTEXTUAL_DATA_BASE}/{id}/test");
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
            "integrations": [
                {
                    "integration": {
                        "id": "cd-001",
                        "name": "GitHub Commits",
                        "description": "Enrich logs with GitHub commit data"
                    },
                    "amountIntegrations": 2,
                    "isDeprecated": false,
                    "isNew": true,
                    "upgradeAvailable": false,
                    "errors": []
                }
            ]
        });
        let resp: ListContextualDataResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.integrations.len(), 1);
        let item: ContextualDataIntegration = resp.integrations.into_iter().next().unwrap().into();
        assert_eq!(item.display_name(), "GitHub Commits");
        assert_eq!(item.id.as_deref(), Some("cd-001"));
        assert_eq!(item.amount_integrations, Some(2));
        assert!(!item.is_deprecated);
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({ "integrations": [] });
        let resp: ListContextualDataResponse = serde_json::from_value(json).unwrap();
        assert!(resp.integrations.is_empty());
    }

    #[test]
    fn deserialize_save_response() {
        let json = json!({ "integrationId": "cd-001" });
        let resp: SaveContextualDataResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.integration_id.as_deref(), Some("cd-001"));
    }

    #[test]
    fn display_missing_fields() {
        let c = ContextualDataIntegration {
            id: None,
            name: None,
            description: None,
            is_deprecated: false,
            amount_integrations: None,
        };
        assert_eq!(c.display_name(), "-");
        assert_eq!(c.display_description(), "-");
    }
}
