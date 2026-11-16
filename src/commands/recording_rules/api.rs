use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;
use crate::serde_helpers::string_or_number;

// --- Response types ---

/// A recording rule group *set* as the v1 REST API returns it:
/// `{id, name, groups: [{id, name, interval, limit, rules: [...]}]}`.
///
/// The older flat shape (`interval` and `rules` directly on the object) is
/// still accepted so existing fixtures keep working.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingRuleGroup {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    /// The API sends the interval as an integer number of seconds; older
    /// fixtures use a string such as `"60s"`. Both are kept as text.
    #[serde(default, deserialize_with = "string_or_number")]
    pub interval: Option<String>,
    pub rules: Option<Vec<Value>>,
    pub groups: Option<Vec<Value>>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl RecordingRuleGroup {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    /// Number of rules in the set: direct `rules` plus every `groups[].rules`.
    pub fn rules_count(&self) -> usize {
        let direct = self.rules.as_ref().map(|r| r.len()).unwrap_or(0);
        let nested: usize = self
            .groups
            .as_ref()
            .map(|groups| {
                groups
                    .iter()
                    .map(|g| {
                        g.get("rules")
                            .and_then(Value::as_array)
                            .map(|r| r.len())
                            .unwrap_or(0)
                    })
                    .sum()
            })
            .unwrap_or(0);
        direct + nested
    }

    /// Evaluation interval: the set's own, else the first group's.
    pub fn display_interval(&self) -> Option<String> {
        if let Some(interval) = &self.interval {
            return Some(interval.clone());
        }
        self.groups
            .as_ref()?
            .first()?
            .get("interval")
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
    }
}

/// `GET /v1` answers `{"sets": [...]}`; older fixtures use `{"groups": [...]}`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRecordingRuleGroupsResponse {
    #[serde(default, alias = "sets")]
    pub groups: Vec<RecordingRuleGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRecordingRuleGroupResponse {
    pub group: Option<RecordingRuleGroup>,
}

/// `POST /v1` answers `{"id": "..."}` only; older fixtures answer
/// `{"group": {...}}`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecordingRuleGroupResponse {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub group: Option<RecordingRuleGroup>,
}

/// `PUT /v1/{id}` answers `{}`; older fixtures answer `{"group": {...}}`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRecordingRuleGroupResponse {
    pub group: Option<RecordingRuleGroup>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteRecordingRuleGroupResponse {}

/// Parse a `GET /v1/{id}` payload. The API returns the set at the top
/// level; older fixtures wrap it as `{"group": {...}}`.
pub fn parse_set(val: Value) -> std::result::Result<RecordingRuleGroup, serde_json::Error> {
    let inner = match val.get("group") {
        Some(group) => group.clone(),
        None => val,
    };
    serde_json::from_value(inner)
}

// --- API ---

const RECORDING_RULES_BASE: &str = "/mgmt/openapi/5/recording-rules/recording-rules/v1";

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

    /// Fetch a set and parse it into a [`RecordingRuleGroup`].
    pub async fn get_set(&self, id: &str) -> Result<RecordingRuleGroup> {
        let val = self.get(id).await?;
        Ok(parse_set(val)?)
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

    /// The real v1 API: `{"sets": [{id, name, groups: [{interval: 60, rules}]}]}`.
    #[test]
    fn deserialize_list_response_sets_shape() {
        let json = json!({
            "sets": [
                {
                    "id": "01M74QGJWQMN8V73EZV31FKADD",
                    "name": "my-set",
                    "groups": [
                        {
                            "id": "01M74QGJWR81HAA2WBJTVRW8FC",
                            "name": "my-set",
                            "interval": 60,
                            "limit": "0",
                            "version": 0,
                            "rules": [
                                {"record": "a", "expr": "vector(1)", "labels": {}},
                                {"record": "b", "expr": "vector(2)", "labels": {}}
                            ]
                        },
                        { "name": "second", "interval": 120, "rules": [{"record": "c", "expr": "1"}] }
                    ]
                }
            ]
        });
        let resp: ListRecordingRuleGroupsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.groups.len(), 1);
        let set = &resp.groups[0];
        assert_eq!(set.id.as_deref(), Some("01M74QGJWQMN8V73EZV31FKADD"));
        assert_eq!(set.display_name(), "my-set");
        assert_eq!(set.rules_count(), 3);
        assert_eq!(set.display_interval().as_deref(), Some("60"));
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

    /// `parse_set` accepts both the wrapped fixture shape and the bare set
    /// that the API really returns.
    #[test]
    fn parse_set_accepts_wrapped_and_bare() {
        let wrapped = json!({ "group": { "id": "rr-001", "name": "wrapped", "rules": [{}] } });
        let set = parse_set(wrapped).unwrap();
        assert_eq!(set.display_name(), "wrapped");
        assert_eq!(set.rules_count(), 1);

        let bare = json!({
            "id": "01M74QHSJFM03JC96ANND27MCT",
            "name": "bare",
            "groups": [{ "name": "bare", "interval": 60, "rules": [{}, {}] }]
        });
        let set = parse_set(bare).unwrap();
        assert_eq!(set.id.as_deref(), Some("01M74QHSJFM03JC96ANND27MCT"));
        assert_eq!(set.display_name(), "bare");
        assert_eq!(set.rules_count(), 2);
        assert_eq!(set.display_interval().as_deref(), Some("60"));
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

    /// The real v1 API answers a create with only the new set id.
    #[test]
    fn deserialize_create_response_id_only() {
        let json = json!({ "id": "01M74QHSJFM03JC96ANND27MCT" });
        let resp: CreateRecordingRuleGroupResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.id.as_deref(), Some("01M74QHSJFM03JC96ANND27MCT"));
        assert!(resp.group.is_none());
    }

    /// The real v1 API answers an update with `{}`.
    #[test]
    fn deserialize_update_response_empty() {
        let resp: UpdateRecordingRuleGroupResponse = serde_json::from_value(json!({})).unwrap();
        assert!(resp.group.is_none());
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteRecordingRuleGroupResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn display_name_missing() {
        let group = RecordingRuleGroup::default();
        assert_eq!(group.display_name(), "-");
        assert_eq!(group.rules_count(), 0);
        assert!(group.display_interval().is_none());
    }
}
