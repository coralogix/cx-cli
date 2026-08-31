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
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
    // `integrationType` is a oneof object whose single key is the type
    // (e.g. `cloudformation`, `arm`, `managed`, `untracked`).
    pub integration_type: Option<Value>,
}

impl Integration {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_type(&self) -> &str {
        self.integration_type
            .as_ref()
            .and_then(|v| v.as_object())
            .and_then(|m| m.keys().next())
            .map(String::as_str)
            .unwrap_or("-")
    }

    pub fn display_version(&self) -> &str {
        self.versions.last().map(String::as_str).unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationEntry {
    pub integration: Integration,
    #[serde(default)]
    pub errors: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListIntegrationsResponse {
    #[serde(default)]
    pub integrations: Vec<IntegrationEntry>,
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
const INTEGRATIONS_METADATA_BASE: &str = "/mgmt/openapi/5/integrations/metadata/v1";

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

    pub async fn update(&self, body: &Value) -> Result<Value> {
        self.client.put(INTEGRATIONS_METADATA_BASE, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteIntegrationResponse> {
        let path = format!("{INTEGRATIONS_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn test(&self, body: &Value) -> Result<Value> {
        let path = format!("{INTEGRATIONS_METADATA_BASE}/test");
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
            "integrations": [
                {
                    "integration": {
                        "id": "aws-sns-shipper",
                        "name": "AWS SNS",
                        "description": "Ship logs via SNS",
                        "tags": ["AWS", "Logs"],
                        "versions": ["0.0.1", "0.0.40"],
                        "integrationType": { "cloudformation": {} }
                    },
                    "errors": []
                }
            ]
        });
        let resp: ListIntegrationsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.integrations.len(), 1);
        let i = &resp.integrations[0].integration;
        assert_eq!(i.id.as_deref(), Some("aws-sns-shipper"));
        assert_eq!(i.display_name(), "AWS SNS");
        assert_eq!(i.display_type(), "cloudformation");
        assert_eq!(i.display_version(), "0.0.40");
        assert_eq!(i.tags, vec!["AWS", "Logs"]);
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({ "integrations": [] });
        let resp: ListIntegrationsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.integrations.is_empty());
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
            description: None,
            tags: vec![],
            versions: vec![],
            integration_type: None,
        };
        assert_eq!(i.display_name(), "-");
        assert_eq!(i.display_type(), "-");
        assert_eq!(i.display_version(), "-");
    }
}
