use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;
use crate::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Enrichment {
    #[serde(default, deserialize_with = "string_or_number")]
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

/// Build `enrichment_ids=<a>&enrichment_ids=<b>`. Ids are numeric in
/// practice; anything outside the URL-unreserved set is percent-encoded.
pub fn remove_query(ids: &[String]) -> String {
    fn encode(id: &str) -> String {
        let mut out = String::with_capacity(id.len());
        for byte in id.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char)
                }
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    }
    ids.iter()
        .map(|id| format!("enrichment_ids={}", encode(id)))
        .collect::<Vec<_>>()
        .join("&")
}

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

    /// `DELETE /v1?enrichment_ids=<id>&enrichment_ids=<id>`. The API takes
    /// the ids as repeated query parameters and ignores a JSON body.
    pub async fn remove(&self, ids: &[String]) -> Result<Value> {
        let path = format!("{ENRICHMENTS_BASE}?{}", remove_query(ids));
        self.client.delete(&path).await
    }

    /// `PUT /v1/all`: atomic overwrite for all enrichment types. The plain
    /// `PUT /v1` is the deprecated custom-only variant with a different body.
    pub async fn overwrite(&self, body: &Value) -> Result<Value> {
        let path = format!("{ENRICHMENTS_BASE}/all");
        self.client.put(&path, body).await
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
    fn remove_query_repeats_the_parameter_and_encodes() {
        let ids = vec!["272627".to_string(), "a b&c".to_string()];
        assert_eq!(
            remove_query(&ids),
            "enrichment_ids=272627&enrichment_ids=a%20b%26c"
        );
        assert_eq!(remove_query(&[]), "");
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
