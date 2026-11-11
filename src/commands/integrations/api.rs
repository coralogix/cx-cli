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

/// `POST /integrations/metadata/v1` answers with just the new deployment id.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveIntegrationResponse {
    pub integration_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteIntegrationResponse {}

// --- API ---

const INTEGRATIONS_BASE: &str = "/mgmt/openapi/5/integrations/integrations/v1";
const INTEGRATIONS_METADATA_BASE: &str = "/mgmt/openapi/5/integrations/metadata/v1";
const INTEGRATIONS_TEMPLATE_BASE: &str = "/mgmt/openapi/5/integrations/template/v1";

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

    /// `GET .../integrations/v1/definitions/{key}` - the catalog definition
    /// (fields per revision) of an integration type.
    pub async fn get_definition(&self, id: &str) -> Result<Value> {
        let path = format!("{INTEGRATIONS_BASE}/definitions/{id}");
        self.client.get(&path, &[]).await
    }

    /// `GET .../integrations/v1/deployed/{instance-id}` - one deployed
    /// instance (its key, version and parameters).
    pub async fn get_deployed(&self, id: &str) -> Result<Value> {
        let path = format!("{INTEGRATIONS_BASE}/deployed/{id}");
        self.client.get(&path, &[]).await
    }

    /// `POST .../integrations/metadata/v1` with `{ "metadata": {...} }`
    /// registers a new deployment and returns its id.
    pub async fn save(&self, body: &Value) -> Result<SaveIntegrationResponse> {
        self.client.post(INTEGRATIONS_METADATA_BASE, body).await
    }

    pub async fn update(&self, body: &Value) -> Result<Value> {
        self.client.put(INTEGRATIONS_METADATA_BASE, body).await
    }

    /// `DELETE .../integrations/v1/instances/{instance-id}`.
    pub async fn delete(&self, id: &str) -> Result<DeleteIntegrationResponse> {
        let path = format!("{INTEGRATIONS_BASE}/instances/{id}");
        self.client.delete(&path).await
    }

    pub async fn test(&self, body: &Value) -> Result<Value> {
        let path = format!("{INTEGRATIONS_METADATA_BASE}/test");
        self.client.post(&path, body).await
    }

    /// `GET .../integrations/template/v1?integration_id=...` renders the
    /// deployment template (e.g. a CloudFormation URL) of a deployed
    /// instance. The API rejects the call without an id.
    pub async fn get_template(&self, integration_id: Option<&str>) -> Result<Value> {
        let params: Vec<(&str, &str)> = integration_id
            .map(|id| vec![("integration_id", id)])
            .unwrap_or_default();
        self.client.get(INTEGRATIONS_TEMPLATE_BASE, &params).await
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
        let json = json!({ "integrationId": "640bd793-7569-4f61-ae6c-351ad9de1fdd" });
        let resp: SaveIntegrationResponse = serde_json::from_value(json).unwrap();
        assert_eq!(
            resp.integration_id.as_deref(),
            Some("640bd793-7569-4f61-ae6c-351ad9de1fdd")
        );
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
