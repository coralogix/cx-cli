use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;
use crate::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Integration {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub integration_type: Option<String>,
    pub status: Option<String>,
    pub version: Option<u32>,
}

impl Integration {
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
pub struct ListIntegrationsResponse {
    #[serde(default)]
    pub deployments: Vec<Integration>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveIntegrationResponse {
    pub deployment: Option<Integration>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteIntegrationResponse {}

// --- API ---

const INTEGRATIONS_BASE: &str = "/mgmt/openapi/5/integrations/integrations/v1";

pub struct IntegrationsApi<'a> {
    client: &'a CxClient,
}

impl<'a> IntegrationsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListIntegrationsResponse> {
        self.client.get(INTEGRATIONS_BASE, &[]).await
    }

    pub async fn get_details(&self, id: &str) -> Result<Value> {
        let path = format!("{INTEGRATIONS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn get_definition(&self, id: &str) -> Result<Value> {
        let path = format!("{INTEGRATIONS_BASE}/{id}/definition");
        self.client.get(&path, &[]).await
    }

    pub async fn get_deployed(&self, id: &str) -> Result<Value> {
        let path = format!("{INTEGRATIONS_BASE}/{id}/deployed");
        self.client.get(&path, &[]).await
    }

    pub async fn save(&self, body: &Value) -> Result<SaveIntegrationResponse> {
        self.client.post(INTEGRATIONS_BASE, body).await
    }

    pub async fn update(&self, id: &str, body: &Value) -> Result<Value> {
        let path = format!("{INTEGRATIONS_BASE}/{id}");
        self.client.put(&path, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteIntegrationResponse> {
        let path = format!("{INTEGRATIONS_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn test(&self, body: &Value) -> Result<Value> {
        let path = format!("{INTEGRATIONS_BASE}/test");
        self.client.post(&path, body).await
    }

    pub async fn get_template(&self) -> Result<Value> {
        let path = format!("{INTEGRATIONS_BASE}/template");
        self.client.get(&path, &[]).await
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
            "deployments": [
                {
                    "id": "int-001",
                    "name": "AWS Integration",
                    "type": "aws",
                    "status": "active",
                    "version": 1
                }
            ]
        });
        let resp: ListIntegrationsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.deployments.len(), 1);
        assert_eq!(resp.deployments[0].display_name(), "AWS Integration");
        assert_eq!(resp.deployments[0].display_type(), "aws");
        assert_eq!(resp.deployments[0].display_status(), "active");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({ "deployments": [] });
        let resp: ListIntegrationsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.deployments.is_empty());
    }

    #[test]
    fn deserialize_save_response() {
        let json = json!({ "deployment": { "id": "int-001", "name": "AWS Integration" } });
        let resp: SaveIntegrationResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.deployment.unwrap().id.as_deref(), Some("int-001"));
    }

    #[test]
    fn display_missing_fields() {
        let i = Integration {
            id: None,
            name: None,
            integration_type: None,
            status: None,
            version: None,
        };
        assert_eq!(i.display_name(), "-");
        assert_eq!(i.display_type(), "-");
        assert_eq!(i.display_status(), "-");
    }
}
