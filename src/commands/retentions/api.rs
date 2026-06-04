use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Retention {
    pub id: Option<String>,
    pub name: Option<String>,
    pub retention_days: Option<u32>,
    pub enabled: Option<bool>,
}

impl Retention {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRetentionsResponse {
    #[serde(default)]
    pub retentions: Vec<Retention>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRetentionsResponse {}

#[derive(Debug, Deserialize)]
pub struct ActivateRetentionResponse {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionStatusResponse {
    pub enabled: Option<bool>,
}

// --- API ---

const RETENTIONS_BASE: &str = "/mgmt/openapi/5/dataengine/retention-tags/v1";

pub struct RetentionsApi<'a> {
    client: &'a CxClient,
}

impl<'a> RetentionsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<Value> {
        self.client.get(RETENTIONS_BASE, &[]).await
    }

    pub async fn update(&self, body: &Value) -> Result<Value> {
        self.client.put(RETENTIONS_BASE, body).await
    }

    pub async fn activate(&self) -> Result<Value> {
        let path = format!("{RETENTIONS_BASE}/activate");
        self.client.post(&path, &serde_json::json!({})).await
    }

    pub async fn status(&self) -> Result<Value> {
        let path = format!("{RETENTIONS_BASE}/enabled");
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
            "retentions": [
                {
                    "id": "ret-001",
                    "name": "Hot Storage",
                    "retentionDays": 30,
                    "enabled": true
                },
                {
                    "id": "ret-002",
                    "name": "Warm Storage",
                    "retentionDays": 90,
                    "enabled": false
                }
            ]
        });

        let resp: ListRetentionsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.retentions.len(), 2);
        assert_eq!(resp.retentions[0].display_name(), "Hot Storage");
        assert_eq!(resp.retentions[0].retention_days, Some(30));
        assert_eq!(resp.retentions[0].enabled, Some(true));
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListRetentionsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.retentions.is_empty());
    }

    #[test]
    fn deserialize_status_response() {
        let json = json!({ "enabled": true });
        let resp: RetentionStatusResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.enabled, Some(true));
    }

    #[test]
    fn display_missing_name() {
        let ret = Retention {
            id: None,
            name: None,
            retention_days: None,
            enabled: None,
        };
        assert_eq!(ret.display_name(), "-");
    }
}
