use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingRuleGroup {
    pub id: Option<String>,
    pub name: Option<String>,
    pub interval: Option<String>,
    pub rules: Option<Vec<Value>>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl RecordingRuleGroup {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn rules_count(&self) -> usize {
        self.rules.as_ref().map(|r| r.len()).unwrap_or(0)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRecordingRuleGroupsResponse {
    #[serde(default)]
    pub groups: Vec<RecordingRuleGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRecordingRuleGroupResponse {
    pub group: Option<RecordingRuleGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecordingRuleGroupResponse {
    pub group: Option<RecordingRuleGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRecordingRuleGroupResponse {
    pub group: Option<RecordingRuleGroup>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteRecordingRuleGroupResponse {}

// --- API ---

const RECORDING_RULES_BASE: &str = "/mgmt/openapi/latest/recording-rules/rule-groups/v1";

pub struct RecordingRulesApi<'a> {
    client: &'a CxClient,
}

impl<'a> RecordingRulesApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListRecordingRuleGroupsResponse> {
        self.client.get(RECORDING_RULES_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{RECORDING_RULES_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateRecordingRuleGroupResponse> {
        self.client.post(RECORDING_RULES_BASE, body).await
    }

    pub async fn update(&self, id: &str, body: &Value) -> Result<UpdateRecordingRuleGroupResponse> {
        let path = format!("{RECORDING_RULES_BASE}/{id}");
        self.client.put(&path, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteRecordingRuleGroupResponse> {
        let path = format!("{RECORDING_RULES_BASE}/{id}");
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
            "groups": [
                {
                    "id": "rr-001",
                    "name": "Error Rate Rules",
                    "interval": "60s",
                    "rules": [
                        {"record": "job:http_errors:rate5m", "expr": "rate(http_errors_total[5m])"}
                    ],
                    "createdAt": "2024-01-01T00:00:00Z"
                },
                {
                    "id": "rr-002",
                    "name": "Latency Rules",
                    "interval": "30s",
                    "rules": []
                }
            ]
        });

        let resp: ListRecordingRuleGroupsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.groups.len(), 2);
        assert_eq!(resp.groups[0].id.as_deref(), Some("rr-001"));
        assert_eq!(resp.groups[0].display_name(), "Error Rate Rules");
        assert_eq!(resp.groups[0].rules_count(), 1);
        assert_eq!(resp.groups[1].rules_count(), 0);
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListRecordingRuleGroupsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.groups.is_empty());
    }

    #[test]
    fn deserialize_get_response() {
        let json = json!({
            "group": {
                "id": "rr-001",
                "name": "Error Rate Rules",
                "interval": "60s",
                "rules": [
                    {"record": "job:http_errors:rate5m", "expr": "rate(http_errors_total[5m])"}
                ]
            }
        });
        let resp: GetRecordingRuleGroupResponse = serde_json::from_value(json).unwrap();
        let group = resp.group.unwrap();
        assert_eq!(group.id.as_deref(), Some("rr-001"));
        assert_eq!(group.rules_count(), 1);
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "group": {
                "id": "rr-new",
                "name": "New Group"
            }
        });
        let resp: CreateRecordingRuleGroupResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.group.unwrap().id.as_deref(), Some("rr-new"));
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteRecordingRuleGroupResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn display_name_missing() {
        let group = RecordingRuleGroup {
            id: None,
            name: None,
            interval: None,
            rules: None,
            created_at: None,
            updated_at: None,
        };
        assert_eq!(group.display_name(), "-");
        assert_eq!(group.rules_count(), 0);
    }
}
