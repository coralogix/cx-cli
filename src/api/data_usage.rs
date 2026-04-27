use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataUsageSummaryResponse {
    #[serde(default)]
    pub usage: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsageResponse {
    #[serde(default)]
    pub data: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsCountResponse {
    #[serde(default)]
    pub count: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpansCountResponse {
    #[serde(default)]
    pub count: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportStatusResponse {
    #[serde(default)]
    pub status: Value,
}

// --- API ---

const DATA_USAGE_BASE: &str = "/mgmt/openapi/latest/dataplans/data-usage/v2";

pub struct DataUsageApi<'a> {
    client: &'a CxClient,
}

impl<'a> DataUsageApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn get_usage(&self, params: &[(&str, &str)]) -> Result<Value> {
        self.client.get(DATA_USAGE_BASE, params).await
    }

    pub async fn daily(&self, data_type: &str, body: &Value) -> Result<Value> {
        let path = format!("{DATA_USAGE_BASE}/daily/{data_type}");
        self.client.post(&path, body).await
    }

    pub async fn logs_count(&self) -> Result<Value> {
        let path = format!("{DATA_USAGE_BASE}/logs/count");
        self.client.get(&path, &[]).await
    }

    pub async fn spans_count(&self) -> Result<Value> {
        let path = format!("{DATA_USAGE_BASE}/spans/count");
        self.client.get(&path, &[]).await
    }

    pub async fn export_status(&self) -> Result<Value> {
        let path = format!("{DATA_USAGE_BASE}/export-status");
        self.client.get(&path, &[]).await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_summary_response() {
        let json = json!({
            "usage": {
                "totalGb": 42.5,
                "logsGb": 30.0,
                "spansGb": 12.5
            }
        });
        let resp: DataUsageSummaryResponse = serde_json::from_value(json).unwrap();
        assert!(resp.usage.get("totalGb").is_some());
    }

    #[test]
    fn deserialize_daily_response() {
        let json = json!({
            "data": [
                {"date": "2024-01-01", "value": 10.5},
                {"date": "2024-01-02", "value": 12.3}
            ]
        });
        let resp: DailyUsageResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.data.len(), 2);
    }

    #[test]
    fn deserialize_empty_daily() {
        let json = json!({});
        let resp: DailyUsageResponse = serde_json::from_value(json).unwrap();
        assert!(resp.data.is_empty());
    }

    #[test]
    fn deserialize_logs_count_response() {
        let json = json!({ "count": 1000000 });
        let resp: LogsCountResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.count, json!(1000000));
    }

    #[test]
    fn deserialize_spans_count_response() {
        let json = json!({ "count": 500000 });
        let resp: SpansCountResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.count, json!(500000));
    }
}
