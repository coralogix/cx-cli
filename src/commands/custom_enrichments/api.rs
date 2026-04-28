use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;
use crate::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEnrichment {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub enrichment_type: Option<String>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
}

impl CustomEnrichment {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_type(&self) -> String {
        self.enrichment_type
            .as_deref()
            .map(|s| {
                s.strip_prefix("CUSTOM_ENRICHMENT_TYPE_")
                    .unwrap_or(s)
                    .to_string()
            })
            .unwrap_or_else(|| "-".to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListCustomEnrichmentsResponse {
    #[serde(default)]
    pub custom_enrichments: Vec<CustomEnrichment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetCustomEnrichmentResponse {
    pub custom_enrichment: Option<CustomEnrichment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCustomEnrichmentResponse {
    pub custom_enrichment: Option<CustomEnrichment>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteCustomEnrichmentResponse {}

// --- API ---

const CUSTOM_ENRICHMENTS_BASE: &str = "/mgmt/openapi/5/enrichment-rules/custom-enrichment-rules/v1";

pub struct CustomEnrichmentsApi<'a> {
    client: &'a CxClient,
}

impl<'a> CustomEnrichmentsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListCustomEnrichmentsResponse> {
        self.client.get(CUSTOM_ENRICHMENTS_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{CUSTOM_ENRICHMENTS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateCustomEnrichmentResponse> {
        self.client.post(CUSTOM_ENRICHMENTS_BASE, body).await
    }

    pub async fn update(&self, body: &Value) -> Result<Value> {
        self.client.put(CUSTOM_ENRICHMENTS_BASE, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteCustomEnrichmentResponse> {
        let path = format!("{CUSTOM_ENRICHMENTS_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn search(&self, id: &str, query: &str) -> Result<Value> {
        let path = format!("{CUSTOM_ENRICHMENTS_BASE}/{id}/search");
        self.client
            .post(&path, &serde_json::json!({"query": query}))
            .await
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
            "customEnrichments": [
                { "id": "ce-001", "name": "IP Lookup", "type": "CUSTOM_ENRICHMENT_TYPE_CSV", "createTime": "2024-01-01T00:00:00Z" },
                { "id": "ce-002", "name": "User DB", "description": "User enrichment table" }
            ]
        });
        let resp: ListCustomEnrichmentsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.custom_enrichments.len(), 2);
        assert_eq!(resp.custom_enrichments[0].display_name(), "IP Lookup");
        assert_eq!(resp.custom_enrichments[0].display_type(), "CSV");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListCustomEnrichmentsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.custom_enrichments.is_empty());
    }

    #[test]
    fn deserialize_get_response() {
        let json = json!({ "customEnrichment": { "id": "ce-001", "name": "IP Lookup" } });
        let resp: GetCustomEnrichmentResponse = serde_json::from_value(json).unwrap();
        assert_eq!(
            resp.custom_enrichment.unwrap().id.as_deref(),
            Some("ce-001")
        );
    }

    #[test]
    fn deserialize_delete_response() {
        let _: DeleteCustomEnrichmentResponse = serde_json::from_value(json!({})).unwrap();
    }

    #[test]
    fn display_missing_fields() {
        let ce = CustomEnrichment {
            id: None,
            name: None,
            description: None,
            enrichment_type: None,
            create_time: None,
            update_time: None,
        };
        assert_eq!(ce.display_name(), "-");
        assert_eq!(ce.display_type(), "-");
    }
}
