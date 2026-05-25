use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResultResponse {
    #[serde(default)]
    pub result: Value,
}

// --- API ---

const NOTIFICATION_TEST_BASE: &str = "/mgmt/openapi/5/notifications/notification-center/v1";

pub struct NotificationTestingApi<'a> {
    client: &'a CxClient,
}

impl<'a> NotificationTestingApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn test_connector(&self, body: &Value) -> Result<Value> {
        let path = format!("{NOTIFICATION_TEST_BASE}/connectors/tests/config");
        self.client.post(&path, body).await
    }

    pub async fn test_destination(&self, body: &Value) -> Result<Value> {
        let path = format!("{NOTIFICATION_TEST_BASE}/destinations/tests");
        self.client.post(&path, body).await
    }

    pub async fn test_preset(&self, body: &Value) -> Result<Value> {
        let path = format!("{NOTIFICATION_TEST_BASE}/presets/tests/config");
        self.client.post(&path, body).await
    }

    pub async fn test_routing_condition(&self, body: &Value) -> Result<Value> {
        let path = format!("{NOTIFICATION_TEST_BASE}/routing-conditions/tests");
        self.client.post(&path, body).await
    }

    pub async fn test_template_render(&self, body: &Value) -> Result<Value> {
        let path = format!("{NOTIFICATION_TEST_BASE}/template-render/tests");
        self.client.post(&path, body).await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_test_result_response() {
        let json = json!({
            "result": {
                "success": true,
                "message": "Connector test passed"
            }
        });
        let resp: TestResultResponse = serde_json::from_value(json).unwrap();
        assert!(resp.result.get("success").is_some());
    }

    #[test]
    fn deserialize_empty_result() {
        let json = json!({});
        let resp: TestResultResponse = serde_json::from_value(json).unwrap();
        assert!(resp.result.is_null());
    }
}
