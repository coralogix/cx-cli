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
    /// The v1 API nests rules as `ruleSubgroups[].rules[]`.
    pub rule_subgroups: Option<Vec<Value>>,
    pub enabled: Option<bool>,
    pub order: Option<u32>,
    pub creator: Option<String>,
    pub description: Option<String>,
}

impl RuleGroup {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    /// Number of rules: direct `rules` plus every `ruleSubgroups[].rules`.
    pub fn rules_count(&self) -> usize {
        let direct = self.rules.as_ref().map(|r| r.len()).unwrap_or(0);
        let nested: usize = self
            .rule_subgroups
            .as_ref()
            .map(|subgroups| {
                subgroups
                    .iter()
                    .map(|sg| {
                        sg.get("rules")
                            .and_then(Value::as_array)
                            .map(|r| r.len())
                            .unwrap_or(0)
                    })
                    .sum()
            })
            .unwrap_or(0);
        direct + nested
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

/// Build `group_ids=<a>&group_ids=<b>`. Ids are UUIDs in practice; anything
/// outside the URL-unreserved set is percent-encoded.
pub fn bulk_delete_query(ids: &[String]) -> String {
    fn encode(id: &str) -> String {
        let mut out = String::with_capacity(id.len());
        for byte in id.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char)
                }
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    }
    ids.iter()
        .map(|id| format!("group_ids={}", encode(id)))
        .collect::<Vec<_>>()
        .join("&")
}

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

    /// `DELETE /v1?group_ids=<id>&group_ids=<id>`. The API takes the ids
    /// as repeated query parameters; there is no `/bulk-delete` resource.
    pub async fn bulk_delete(&self, ids: &[String]) -> Result<Value> {
        let path = format!("{RULE_GROUPS_BASE}?{}", bulk_delete_query(ids));
        self.client.delete(&path).await
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
    fn bulk_delete_query_repeats_the_parameter_and_encodes() {
        let ids = vec![
            "6096268d-b840-4c3f-9406-12b32b9efb6a".to_string(),
            "a b".to_string(),
        ];
        assert_eq!(
            bulk_delete_query(&ids),
            "group_ids=6096268d-b840-4c3f-9406-12b32b9efb6a&group_ids=a%20b"
        );
        assert_eq!(bulk_delete_query(&[]), "");
    }

    #[test]
    fn display_missing_fields() {
        let rg = RuleGroup {
            id: None,
            name: None,
            rules: None,
            rule_subgroups: None,
            enabled: None,
            order: None,
            creator: None,
            description: None,
        };
        assert_eq!(rg.display_name(), "-");
        assert_eq!(rg.rules_count(), 0);
    }

    /// The v1 API nests rules under `ruleSubgroups[].rules[]`.
    #[test]
    fn rules_count_includes_subgroups() {
        let json = json!({
            "id": "rg-1",
            "name": "Severity Rules",
            "ruleSubgroups": [
                { "order": 1, "rules": [{"name": "a"}, {"name": "b"}] },
                { "order": 2, "rules": [{"name": "c"}] },
                { "order": 3 }
            ]
        });
        let rg: RuleGroup = serde_json::from_value(json).unwrap();
        assert_eq!(rg.rules_count(), 3);
    }
}
