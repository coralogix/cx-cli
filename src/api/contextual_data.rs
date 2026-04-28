use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;
use super::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextualDataIntegration {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub integration_type: Option<String>,
    pub status: Option<String>,
    pub created_at: Option<String>,
}

impl ContextualDataIntegration {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_type(&self) -> &str {
        self.integration_type.as_deref().unwrap_or("-")
    }

    pub fn display_status(&self) -> &str {
        self.status.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListContextualDataResponse {
    #[serde(default)]
    pub integrations: Vec<ContextualDataIntegration>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveContextualDataResponse {
    pub integration: Option<ContextualDataIntegration>,
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
                { "id": "cd-001", "name": "GitHub Commits", "type": "github", "status": "active" }
            ]
        });
        let resp: ListContextualDataResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.integrations.len(), 1);
        assert_eq!(resp.integrations[0].display_name(), "GitHub Commits");
        assert_eq!(resp.integrations[0].display_status(), "active");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({ "integrations": [] });
        let resp: ListContextualDataResponse = serde_json::from_value(json).unwrap();
        assert!(resp.integrations.is_empty());
    }

    #[test]
    fn deserialize_save_response() {
        let json = json!({ "integration": { "id": "cd-001", "name": "GitHub Commits" } });
        let resp: SaveContextualDataResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.integration.unwrap().id.as_deref(), Some("cd-001"));
    }

    #[test]
    fn display_missing_fields() {
        let c = ContextualDataIntegration {
            id: None,
            name: None,
            integration_type: None,
            status: None,
            created_at: None,
        };
        assert_eq!(c.display_name(), "-");
        assert_eq!(c.display_type(), "-");
        assert_eq!(c.display_status(), "-");
    }
}
