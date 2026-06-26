use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;
use crate::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleGroup {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    pub rules: Option<Vec<Value>>,
    pub enabled: Option<bool>,
    pub order: Option<u32>,
    pub creator: Option<String>,
    pub description: Option<String>,
}

impl RuleGroup {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn rules_count(&self) -> usize {
        self.rules.as_ref().map(|r| r.len()).unwrap_or(0)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRuleGroupsResponse {
    #[serde(default)]
    pub rule_groups: Vec<RuleGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRuleGroupResponse {
    pub rule_group: Option<RuleGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRuleGroupResponse {
    pub rule_group: Option<RuleGroup>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteRuleGroupResponse {}

// --- API ---

const RULE_GROUPS_BASE: &str = "/mgmt/openapi/5/parsing-rules/rule-groups/v1";
const PARSING_RULES_LIMITS_BASE: &str = "/mgmt/openapi/5/parsing-rules/limits/v1";

pub struct RuleGroupsApi<'a> {
    client: &'a CxClient,
}

impl<'a> RuleGroupsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListRuleGroupsResponse> {
        self.client.get(RULE_GROUPS_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{RULE_GROUPS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateRuleGroupResponse> {
        self.client.post(RULE_GROUPS_BASE, body).await
    }

    pub async fn update(&self, id: &str, body: &Value) -> Result<Value> {
        let path = format!("{RULE_GROUPS_BASE}/{id}");
        self.client.put(&path, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteRuleGroupResponse> {
        let path = format!("{RULE_GROUPS_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn bulk_delete(&self, body: &Value) -> Result<Value> {
        let path = format!("{RULE_GROUPS_BASE}/bulk-delete");
        self.client.post(&path, body).await
    }

    pub async fn usage_limits(&self) -> Result<Value> {
        self.client.post_empty(PARSING_RULES_LIMITS_BASE, &[]).await
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
            "ruleGroups": [
                {
                    "id": "rg-001",
                    "name": "JSON Parser",
                    "rules": [{"id": "r1"}, {"id": "r2"}],
                    "enabled": true,
                    "order": 1,
                    "creator": "user@example.com"
                }
            ]
        });
        let resp: ListRuleGroupsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.rule_groups.len(), 1);
        assert_eq!(resp.rule_groups[0].display_name(), "JSON Parser");
        assert_eq!(resp.rule_groups[0].rules_count(), 2);
        assert_eq!(resp.rule_groups[0].enabled, Some(true));
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListRuleGroupsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.rule_groups.is_empty());
    }

    #[test]
    fn deserialize_get_response() {
        let json = json!({ "ruleGroup": { "id": "rg-001", "name": "JSON Parser" } });
        let resp: GetRuleGroupResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.rule_group.unwrap().id.as_deref(), Some("rg-001"));
    }

    #[test]
    fn deserialize_delete_response() {
        let _: DeleteRuleGroupResponse = serde_json::from_value(json!({})).unwrap();
    }

    #[test]
    fn display_missing_fields() {
        let rg = RuleGroup {
            id: None,
            name: None,
            rules: None,
            enabled: None,
            order: None,
            creator: None,
            description: None,
        };
        assert_eq!(rg.display_name(), "-");
        assert_eq!(rg.rules_count(), 0);
    }
}
