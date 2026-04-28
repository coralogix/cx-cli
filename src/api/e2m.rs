use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;
use super::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct E2mDefinition {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub e2m_type: Option<String>,
    pub metric_name: Option<String>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
    pub is_active: Option<bool>,
}

impl E2mDefinition {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_type(&self) -> String {
        self.e2m_type
            .as_deref()
            .map(|s| s.strip_prefix("E2M_TYPE_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListE2mResponse {
    #[serde(default)]
    pub e2m: Vec<E2mDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetE2mResponse {
    pub e2m: Option<E2mDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateE2mResponse {
    pub e2m: Option<E2mDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceE2mResponse {
    pub e2m: Option<E2mDefinition>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteE2mResponse {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct E2mLabelsCardinalityResponse {
    #[serde(default)]
    pub labels: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct E2mLimitsResponse {
    pub limit: Option<u64>,
    pub used: Option<u64>,
}

// --- API ---

const E2M_BASE: &str = "/mgmt/openapi/latest/events2metrics/events2metrics/v2";

pub struct E2mApi<'a> {
    client: &'a CxClient,
}

impl<'a> E2mApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListE2mResponse> {
        self.client.get(E2M_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{E2M_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateE2mResponse> {
        self.client.post(E2M_BASE, body).await
    }

    pub async fn replace(&self, body: &Value) -> Result<ReplaceE2mResponse> {
        self.client.put(E2M_BASE, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteE2mResponse> {
        let path = format!("{E2M_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn labels_cardinality(&self) -> Result<E2mLabelsCardinalityResponse> {
        self.client
            .get(
                "/mgmt/openapi/latest/events2metrics/labels/v2/cardinalities",
                &[],
            )
            .await
    }

    pub async fn limits(&self) -> Result<E2mLimitsResponse> {
        self.client
            .get("/mgmt/openapi/latest/events2metrics/limits/v2", &[])
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
            "e2m": [
                {
                    "id": "e2m-001",
                    "name": "Error Count Metric",
                    "type": "E2M_TYPE_LOGS2METRICS",
                    "metricName": "error_count_total",
                    "createTime": "2024-01-01T00:00:00Z",
                    "isActive": true
                },
                {
                    "id": "e2m-002",
                    "name": "Span Duration",
                    "type": "E2M_TYPE_SPANS2METRICS",
                    "metricName": "span_duration_seconds"
                }
            ]
        });

        let resp: ListE2mResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.e2m.len(), 2);
        assert_eq!(resp.e2m[0].id.as_deref(), Some("e2m-001"));
        assert_eq!(resp.e2m[0].display_name(), "Error Count Metric");
        assert_eq!(resp.e2m[0].display_type(), "LOGS2METRICS");
        assert_eq!(resp.e2m[0].is_active, Some(true));
        assert_eq!(resp.e2m[1].display_type(), "SPANS2METRICS");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListE2mResponse = serde_json::from_value(json).unwrap();
        assert!(resp.e2m.is_empty());
    }

    #[test]
    fn deserialize_get_response() {
        let json = json!({
            "e2m": {
                "id": "e2m-001",
                "name": "Error Count Metric",
                "metricName": "error_count_total"
            }
        });
        let resp: GetE2mResponse = serde_json::from_value(json).unwrap();
        let def = resp.e2m.unwrap();
        assert_eq!(def.id.as_deref(), Some("e2m-001"));
        assert_eq!(def.metric_name.as_deref(), Some("error_count_total"));
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "e2m": {
                "id": "e2m-new",
                "name": "New E2M"
            }
        });
        let resp: CreateE2mResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.e2m.unwrap().id.as_deref(), Some("e2m-new"));
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteE2mResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn deserialize_limits_response() {
        let json = json!({
            "limit": 100,
            "used": 42
        });
        let resp: E2mLimitsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.limit, Some(100));
        assert_eq!(resp.used, Some(42));
    }

    #[test]
    fn display_missing_fields() {
        let def = E2mDefinition {
            id: None,
            name: None,
            description: None,
            e2m_type: None,
            metric_name: None,
            create_time: None,
            update_time: None,
            is_active: None,
        };
        assert_eq!(def.display_name(), "-");
        assert_eq!(def.display_type(), "-");
    }
}
