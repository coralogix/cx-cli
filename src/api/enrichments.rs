use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Enrichment {
    pub id: Option<String>,
    pub field_name: Option<String>,
    pub enrichment_type: Option<String>,
    pub source: Option<Value>,
}

impl Enrichment {
    pub fn display_field_name(&self) -> &str {
        self.field_name.as_deref().unwrap_or("-")
    }

    pub fn display_type(&self) -> String {
        self.enrichment_type
            .as_deref()
            .map(|s| s.strip_prefix("ENRICHMENT_TYPE_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEnrichmentsResponse {
    #[serde(default)]
    pub enrichments: Vec<Enrichment>,
}

// --- API ---

const ENRICHMENTS_BASE: &str = "/mgmt/openapi/5/enrichment-rules/enrichment-rules/v1";

pub struct EnrichmentsApi<'a> {
    client: &'a CxClient,
}

impl<'a> EnrichmentsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<Value> {
        self.client.get(ENRICHMENTS_BASE, &[]).await
    }

    pub async fn add(&self, body: &Value) -> Result<Value> {
        self.client.post(ENRICHMENTS_BASE, body).await
    }

    pub async fn remove(&self, body: &Value) -> Result<Value> {
        self.client.delete_with_body(ENRICHMENTS_BASE, body).await
    }

    pub async fn overwrite(&self, body: &Value) -> Result<Value> {
        self.client.put(ENRICHMENTS_BASE, body).await
    }

    pub async fn limit(&self) -> Result<Value> {
        let path = format!("{ENRICHMENTS_BASE}/limit");
        self.client.get(&path, &[]).await
    }

    pub async fn settings(&self) -> Result<Value> {
        let path = format!("{ENRICHMENTS_BASE}/settings");
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
            "enrichments": [
                { "id": "enr-001", "fieldName": "hostname", "enrichmentType": "ENRICHMENT_TYPE_GEO_IP" },
                { "id": "enr-002", "fieldName": "user_id", "enrichmentType": "ENRICHMENT_TYPE_CUSTOM" }
            ]
        });
        let resp: ListEnrichmentsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.enrichments.len(), 2);
        assert_eq!(resp.enrichments[0].display_field_name(), "hostname");
        assert_eq!(resp.enrichments[0].display_type(), "GEO_IP");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListEnrichmentsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.enrichments.is_empty());
    }

    #[test]
    fn display_missing_fields() {
        let e = Enrichment {
            id: None,
            field_name: None,
            enrichment_type: None,
            source: None,
        };
        assert_eq!(e.display_field_name(), "-");
        assert_eq!(e.display_type(), "-");
    }
}
