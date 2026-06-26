use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertSchedulerRule {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub schedule: Option<Value>,
    pub meta_labels: Option<Vec<Value>>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBulkAlertSchedulerRuleResponse {
    #[serde(default)]
    pub alert_scheduler_rules: Vec<AlertSchedulerRule>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAlertSchedulerRuleResponse {
    pub alert_scheduler_rule: Option<AlertSchedulerRule>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertSchedulerRuleResponse {
    pub alert_scheduler_rule: Option<AlertSchedulerRule>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteAlertSchedulerRuleResponse {}

// --- API ---

const SCHEDULERS_BASE: &str = "/mgmt/openapi/5/alerts/suppression-rules/v1";

pub struct AlertSchedulersApi<'a> {
    client: &'a CxClient,
}

impl<'a> AlertSchedulersApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<GetBulkAlertSchedulerRuleResponse> {
        self.client.get(SCHEDULERS_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{SCHEDULERS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateAlertSchedulerRuleResponse> {
        self.client.post(SCHEDULERS_BASE, body).await
    }

    pub async fn update(&self, body: &Value) -> Result<UpdateAlertSchedulerRuleResponse> {
        self.client.put(SCHEDULERS_BASE, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteAlertSchedulerRuleResponse> {
        let path = format!("{SCHEDULERS_BASE}/{id}");
        self.client.delete(&path).await
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
            "alertSchedulerRules": [
                {
                    "id": "rule-1",
                    "name": "Maintenance Window",
                    "enabled": true,
                    "createdAt": "2026-01-01T00:00:00Z"
                }
            ]
        });
        let resp: GetBulkAlertSchedulerRuleResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.alert_scheduler_rules.len(), 1);
        assert_eq!(
            resp.alert_scheduler_rules[0].name.as_deref(),
            Some("Maintenance Window")
        );
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: GetBulkAlertSchedulerRuleResponse = serde_json::from_value(json).unwrap();
        assert!(resp.alert_scheduler_rules.is_empty());
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "alertSchedulerRule": {
                "id": "new-rule",
                "name": "New Rule"
            }
        });
        let resp: CreateAlertSchedulerRuleResponse = serde_json::from_value(json).unwrap();
        let rule = resp.alert_scheduler_rule.unwrap();
        assert_eq!(rule.id.as_deref(), Some("new-rule"));
    }
}
