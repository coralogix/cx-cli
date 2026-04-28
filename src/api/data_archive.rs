use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsArchiveConfig {
    pub id: Option<String>,
    pub bucket: Option<String>,
    pub region: Option<String>,
    pub enabled: Option<bool>,
}

// --- API ---

const METRICS_BASE: &str = "/mgmt/openapi/5/metrics/data-setup/v1";
const LOGS_BASE: &str = "/mgmt/openapi/5/logs/data-setup/v2";

pub struct DataArchiveApi<'a> {
    client: &'a CxClient,
}

impl<'a> DataArchiveApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    // --- Metrics ---

    pub async fn get_config(&self) -> Result<Value> {
        self.client.get(METRICS_BASE, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<Value> {
        self.client.post(METRICS_BASE, body).await
    }

    pub async fn update(&self, body: &Value) -> Result<Value> {
        self.client.put(METRICS_BASE, body).await
    }

    pub async fn enable(&self) -> Result<Value> {
        let path = format!("{METRICS_BASE}/enable");
        self.client.post(&path, &Value::Object(Default::default())).await
    }

    pub async fn disable(&self) -> Result<Value> {
        let path = format!("{METRICS_BASE}/disable");
        self.client.post(&path, &Value::Object(Default::default())).await
    }

    pub async fn validate(&self, body: &Value) -> Result<Value> {
        let path = format!("{METRICS_BASE}/validate");
        self.client.post(&path, body).await
    }

    // --- Logs ---

    pub async fn get_target(&self) -> Result<Value> {
        self.client.get(LOGS_BASE, &[]).await
    }

    pub async fn set_target(&self, body: &Value) -> Result<Value> {
        self.client.post(LOGS_BASE, body).await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_metrics_config() {
        let json = json!({
            "id": "archive-001",
            "bucket": "my-bucket",
            "region": "us-east-1",
            "enabled": true
        });

        let config: MetricsArchiveConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.id.as_deref(), Some("archive-001"));
        assert_eq!(config.bucket.as_deref(), Some("my-bucket"));
        assert_eq!(config.region.as_deref(), Some("us-east-1"));
        assert_eq!(config.enabled, Some(true));
    }

    #[test]
    fn deserialize_metrics_config_minimal() {
        let json = json!({});
        let config: MetricsArchiveConfig = serde_json::from_value(json).unwrap();
        assert!(config.id.is_none());
        assert!(config.bucket.is_none());
    }
}
