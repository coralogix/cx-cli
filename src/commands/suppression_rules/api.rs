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

/// Each element of the live API's `alertSchedulerRules` array arrives wrapped:
/// `{ "alertSchedulerRule": { ... }, "nextActiveTimeframes": [ ... ] }`.
/// Deserializing the element directly as `AlertSchedulerRule` made every field
/// miss, and the all-`Option` struct silently produced rules with null fields.
/// Accept both the wrapped (live) and flat (legacy/fixture) shapes.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ListedAlertSchedulerRule {
    Wrapped {
        #[serde(rename = "alertSchedulerRule")]
        rule: AlertSchedulerRule,
    },
    Flat(AlertSchedulerRule),
}

fn unwrap_rule_envelopes<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<AlertSchedulerRule>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let items = Vec::<ListedAlertSchedulerRule>::deserialize(deserializer)?;
    Ok(items
        .into_iter()
        .map(|item| match item {
            ListedAlertSchedulerRule::Wrapped { rule } => rule,
            ListedAlertSchedulerRule::Flat(rule) => rule,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBulkAlertSchedulerRuleResponse {
    #[serde(default, deserialize_with = "unwrap_rule_envelopes")]
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
    fn deserialize_list_response_flat_legacy_shape() {
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

    // Real live-API shape (us1, July 2026): each element wraps the rule under
    // "alertSchedulerRule" alongside "nextActiveTimeframes". Regression test for
    // list rendering every field as null/empty.
    #[test]
    fn deserialize_list_response_wrapped_live_shape() {
        let json = json!({
            "alertSchedulerRules": [
                {
                    "alertSchedulerRule": {
                        "uniqueIdentifier": "1a9f690e-2d08-4992-8529-59ff5630592f",
                        "id": "faba7566-0e31-4e5e-9813-5e2d02fbc5f8",
                        "name": "Maintenance window mute",
                        "description": "one-time mute",
                        "metaLabels": [],
                        "filter": {
                            "whatExpression": "source logs | filter true",
                            "alertUniqueIds": { "value": ["033848bf-942a-4de9-954e-64ce3e27af51"] }
                        },
                        "schedule": {
                            "scheduleOperation": "SCHEDULE_OPERATION_MUTE",
                            "oneTime": {
                                "timeframe": {
                                    "startTime": "2026-07-17T00:00:00.000",
                                    "endTime": "2026-07-19T23:59:00.000",
                                    "timezone": "UTC-4"
                                }
                            }
                        },
                        "enabled": true,
                        "createdAt": "2026-07-18T04:47:58.000Z",
                        "updatedAt": "2026-07-18T04:47:58.000Z"
                    },
                    "nextActiveTimeframes": []
                }
            ],
            "nextPageToken": ""
        });
        let resp: GetBulkAlertSchedulerRuleResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.alert_scheduler_rules.len(), 1);
        let rule = &resp.alert_scheduler_rules[0];
        assert_eq!(
            rule.id.as_deref(),
            Some("faba7566-0e31-4e5e-9813-5e2d02fbc5f8")
        );
        assert_eq!(rule.name.as_deref(), Some("Maintenance window mute"));
        assert_eq!(rule.enabled, Some(true));
        assert_eq!(rule.created_at.as_deref(), Some("2026-07-18T04:47:58.000Z"));
        assert!(rule.schedule.is_some());
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
