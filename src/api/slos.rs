use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;
use super::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slo {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub creator: Option<String>,
    pub target_threshold_percentage: Option<f64>,
    pub slo_type: Option<String>,
    pub slo_time_frame: Option<String>,
    pub product_type: Option<String>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
}

impl Slo {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_target(&self) -> String {
        self.target_threshold_percentage
            .map(|t| format!("{t}%"))
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn display_type(&self) -> String {
        self.slo_type
            .as_deref()
            .map(|s| s.strip_prefix("SLO_TYPE_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn display_time_frame(&self) -> String {
        self.slo_time_frame
            .as_deref()
            .map(|s| {
                s.strip_prefix("SLO_TIME_FRAME_")
                    .unwrap_or(s)
                    .replace('_', " ")
            })
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn display_product_type(&self) -> String {
        self.product_type
            .as_deref()
            .map(|s| s.strip_prefix("SLO_PRODUCT_TYPE_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSlosResponse {
    #[serde(default)]
    pub slos: Vec<Slo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSloResponse {
    pub slo: Option<Slo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSloResponse {
    pub slo: Option<Slo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceSloResponse {
    pub slo: Option<Slo>,
    #[serde(default)]
    pub effected_slo_alert_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSloResponse {
    #[serde(default)]
    pub effected_slo_alert_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchGetSlosResponse {
    #[serde(default)]
    pub slos: Vec<Slo>,
    #[serde(default)]
    pub not_found_ids: Vec<String>,
}

// --- API ---

const SLO_BASE: &str = "/mgmt/openapi/5/slo/slos/v1";

pub struct SlosApi<'a> {
    client: &'a CxClient,
}

impl<'a> SlosApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListSlosResponse> {
        self.client.get(SLO_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{SLO_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateSloResponse> {
        self.client.post(SLO_BASE, body).await
    }

    pub async fn replace(&self, body: &Value) -> Result<ReplaceSloResponse> {
        self.client.put(SLO_BASE, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteSloResponse> {
        let path = format!("{SLO_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn batch_get(&self, ids: &[&str]) -> Result<BatchGetSlosResponse> {
        let params: Vec<(&str, &str)> = ids.iter().map(|id| ("ids", *id)).collect();
        let path = format!("{SLO_BASE}/all/list");
        self.client.get(&path, &params).await
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
            "slos": [
                {
                    "id": "slo-001",
                    "name": "API Availability",
                    "description": "Tracks API uptime",
                    "creator": "user@example.com",
                    "targetThresholdPercentage": 99.9,
                    "sloType": "SLO_TYPE_REQUEST",
                    "sloTimeFrame": "SLO_TIME_FRAME_28_DAYS",
                    "productType": "SLO_PRODUCT_TYPE_APM",
                    "createTime": "2024-01-01T00:00:00Z",
                    "updateTime": "2024-06-01T12:00:00Z"
                },
                {
                    "id": "slo-002",
                    "name": "Latency SLO",
                    "targetThresholdPercentage": 95.0,
                    "sloType": "SLO_TYPE_WINDOW"
                }
            ]
        });

        let resp: ListSlosResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.slos.len(), 2);
        assert_eq!(resp.slos[0].id.as_deref(), Some("slo-001"));
        assert_eq!(resp.slos[0].name.as_deref(), Some("API Availability"));
        assert_eq!(resp.slos[0].target_threshold_percentage, Some(99.9));
        assert_eq!(resp.slos[0].slo_type.as_deref(), Some("SLO_TYPE_REQUEST"));
        assert_eq!(resp.slos[1].id.as_deref(), Some("slo-002"));
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListSlosResponse = serde_json::from_value(json).unwrap();
        assert!(resp.slos.is_empty());
    }

    #[test]
    fn deserialize_get_response() {
        let json = json!({
            "slo": {
                "id": "slo-001",
                "name": "API Availability"
            }
        });
        let resp: GetSloResponse = serde_json::from_value(json).unwrap();
        let slo = resp.slo.unwrap();
        assert_eq!(slo.id.as_deref(), Some("slo-001"));
        assert_eq!(slo.name.as_deref(), Some("API Availability"));
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "slo": {
                "id": "slo-new",
                "name": "New SLO"
            }
        });
        let resp: CreateSloResponse = serde_json::from_value(json).unwrap();
        let slo = resp.slo.unwrap();
        assert_eq!(slo.id.as_deref(), Some("slo-new"));
    }

    #[test]
    fn deserialize_replace_response() {
        let json = json!({
            "slo": {
                "id": "slo-001",
                "name": "Updated SLO"
            },
            "effectedSloAlertIds": ["alert-1", "alert-2"]
        });
        let resp: ReplaceSloResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.slo.unwrap().name.as_deref(), Some("Updated SLO"));
        assert_eq!(resp.effected_slo_alert_ids.len(), 2);
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({
            "effectedSloAlertIds": ["alert-1"]
        });
        let resp: DeleteSloResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.effected_slo_alert_ids.len(), 1);
    }

    #[test]
    fn deserialize_delete_response_empty() {
        let json = json!({});
        let resp: DeleteSloResponse = serde_json::from_value(json).unwrap();
        assert!(resp.effected_slo_alert_ids.is_empty());
    }

    #[test]
    fn deserialize_batch_get_response() {
        let json = json!({
            "slos": [
                { "id": "slo-001", "name": "SLO A" }
            ],
            "notFoundIds": ["slo-999"]
        });
        let resp: BatchGetSlosResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.slos.len(), 1);
        assert_eq!(resp.not_found_ids, vec!["slo-999"]);
    }

    #[test]
    fn display_name_present() {
        let slo = Slo {
            id: None,
            name: Some("My SLO".to_string()),
            description: None,
            creator: None,
            target_threshold_percentage: None,
            slo_type: None,
            slo_time_frame: None,
            product_type: None,
            create_time: None,
            update_time: None,
        };
        assert_eq!(slo.display_name(), "My SLO");
    }

    #[test]
    fn display_name_missing() {
        let slo = Slo {
            id: None,
            name: None,
            description: None,
            creator: None,
            target_threshold_percentage: None,
            slo_type: None,
            slo_time_frame: None,
            product_type: None,
            create_time: None,
            update_time: None,
        };
        assert_eq!(slo.display_name(), "-");
    }

    #[test]
    fn display_target_present() {
        let slo = Slo {
            id: None,
            name: None,
            description: None,
            creator: None,
            target_threshold_percentage: Some(99.9),
            slo_type: None,
            slo_time_frame: None,
            product_type: None,
            create_time: None,
            update_time: None,
        };
        assert_eq!(slo.display_target(), "99.9%");
    }

    #[test]
    fn display_target_missing() {
        let slo = Slo {
            id: None,
            name: None,
            description: None,
            creator: None,
            target_threshold_percentage: None,
            slo_type: None,
            slo_time_frame: None,
            product_type: None,
            create_time: None,
            update_time: None,
        };
        assert_eq!(slo.display_target(), "-");
    }

    #[test]
    fn display_type_strips_prefix() {
        let slo = Slo {
            id: None,
            name: None,
            description: None,
            creator: None,
            target_threshold_percentage: None,
            slo_type: Some("SLO_TYPE_REQUEST".to_string()),
            slo_time_frame: None,
            product_type: None,
            create_time: None,
            update_time: None,
        };
        assert_eq!(slo.display_type(), "REQUEST");
    }

    #[test]
    fn display_time_frame_strips_prefix() {
        let slo = Slo {
            id: None,
            name: None,
            description: None,
            creator: None,
            target_threshold_percentage: None,
            slo_type: None,
            slo_time_frame: Some("SLO_TIME_FRAME_28_DAYS".to_string()),
            product_type: None,
            create_time: None,
            update_time: None,
        };
        assert_eq!(slo.display_time_frame(), "28 DAYS");
    }

    #[test]
    fn display_product_type_strips_prefix() {
        let slo = Slo {
            id: None,
            name: None,
            description: None,
            creator: None,
            target_threshold_percentage: None,
            slo_type: None,
            slo_time_frame: None,
            product_type: Some("SLO_PRODUCT_TYPE_APM".to_string()),
            create_time: None,
            update_time: None,
        };
        assert_eq!(slo.display_product_type(), "APM");
    }

    #[test]
    fn display_missing_fields() {
        let slo = Slo {
            id: None,
            name: None,
            description: None,
            creator: None,
            target_threshold_percentage: None,
            slo_type: None,
            slo_time_frame: None,
            product_type: None,
            create_time: None,
            update_time: None,
        };
        assert_eq!(slo.display_type(), "-");
        assert_eq!(slo.display_time_frame(), "-");
        assert_eq!(slo.display_product_type(), "-");
    }
}
