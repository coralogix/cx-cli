use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaRuleSet {
    #[serde(default)]
    pub rules: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateQuotaRulesResponse {}

#[derive(Debug, Deserialize)]
pub struct DeleteQuotaRulesResponse {}

// --- API ---

const QUOTA_RULES_BASE: &str = "/mgmt/openapi/latest/dataplans/quota-rules/v1";

pub struct QuotaRulesApi<'a> {
    client: &'a CxClient,
}

impl<'a> QuotaRulesApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn get(&self) -> Result<Value> {
        self.client.get(QUOTA_RULES_BASE, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<Value> {
        self.client.post(QUOTA_RULES_BASE, body).await
    }

    pub async fn replace(&self, body: &Value) -> Result<Value> {
        self.client.put(QUOTA_RULES_BASE, body).await
    }

    pub async fn delete(&self) -> Result<DeleteQuotaRulesResponse> {
        self.client.delete(QUOTA_RULES_BASE).await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_rule_set() {
        let json = json!({
            "rules": [
                {"id": "rule-001", "name": "Team A Quota", "limit": 100},
                {"id": "rule-002", "name": "Team B Quota", "limit": 200}
            ]
        });
        let resp: QuotaRuleSet = serde_json::from_value(json).unwrap();
        assert_eq!(resp.rules.len(), 2);
    }

    #[test]
    fn deserialize_empty_rule_set() {
        let json = json!({});
        let resp: QuotaRuleSet = serde_json::from_value(json).unwrap();
        assert!(resp.rules.is_empty());
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteQuotaRulesResponse = serde_json::from_value(json).unwrap();
    }
}
