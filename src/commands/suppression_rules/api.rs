use serde::Deserialize;
use serde_json::Value;

use crate::error::{CxError, Result};

use crate::api_client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertSchedulerRule {
    /// Stable id; the one every route and console link takes.
    pub unique_identifier: Option<String>,
    /// Version id; changes on every update and isn't addressable.
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub schedule: Option<Value>,
    pub meta_labels: Option<Vec<Value>>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// List items wrap the rule: `{"alertSchedulerRule": {...}, "nextActiveTimeframes": [...]}`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertSchedulerRuleEntry {
    pub alert_scheduler_rule: Option<AlertSchedulerRule>,
    #[serde(default)]
    pub next_active_timeframes: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBulkAlertSchedulerRuleResponse {
    #[serde(default)]
    pub alert_scheduler_rules: Vec<AlertSchedulerRuleEntry>,
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

    /// `None` on 404 or an empty body (pre-CX-57145 backends answer 200 `{}`).
    pub async fn get(&self, id: &str) -> Result<Option<Value>> {
        let path = format!("{SCHEDULERS_BASE}/{id}");
        match self.client.get::<Value>(&path, &[]).await {
            Ok(val) => Ok(rule_found(&val).then_some(val)),
            Err(CxError::Api { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn create(&self, body: &Value) -> Result<CreateAlertSchedulerRuleResponse> {
        self.client.post(SCHEDULERS_BASE, body).await
    }

    pub async fn update(&self, body: &Value) -> Result<UpdateAlertSchedulerRuleResponse> {
        self.client.put(SCHEDULERS_BASE, body).await
    }

    /// `false` if no rule has this `unique_identifier` (404).
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let path = format!("{SCHEDULERS_BASE}/{id}");
        match self
            .client
            .delete::<DeleteAlertSchedulerRuleResponse>(&path)
            .await
        {
            Ok(_) => Ok(true),
            Err(CxError::Api { status: 404, .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// Whether a `GET` body actually carries a rule.
pub fn rule_found(val: &Value) -> bool {
    val.get("alertSchedulerRule")
        .is_some_and(|r| r.is_object() && r.as_object().is_some_and(|m| !m.is_empty()))
}

/// How an input id relates to the rules that actually exist.
#[derive(Debug, PartialEq)]
pub enum RuleIdKind {
    Addressable,
    /// Carries the matching rule's `unique_identifier`.
    VersionId(String),
    Unknown,
}

/// `Addressable` wins if an id matches both fields.
pub fn classify_rule_id_in(resp: &GetBulkAlertSchedulerRuleResponse, input: &str) -> RuleIdKind {
    let mut version_match: Option<String> = None;
    for rule in resp
        .alert_scheduler_rules
        .iter()
        .filter_map(|e| e.alert_scheduler_rule.as_ref())
    {
        if rule.unique_identifier.as_deref() == Some(input) {
            return RuleIdKind::Addressable;
        }
        if rule.id.as_deref() == Some(input) {
            version_match = rule.unique_identifier.clone();
        }
    }
    version_match.map_or(RuleIdKind::Unknown, RuleIdKind::VersionId)
}

/// Costs one `list` call; the only way to map a version id to its rule.
pub async fn classify_rule_id(api: &AlertSchedulersApi<'_>, input: &str) -> Result<RuleIdKind> {
    Ok(classify_rule_id_in(&api.list().await?, input))
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Captured from a live list response.
    fn list_response_fixture() -> Value {
        json!({
            "alertSchedulerRules": [
                {
                    "alertSchedulerRule": {
                        "uniqueIdentifier": "38c4a964-a237-41ea-9b02-87af3d734571",
                        "id": "04b68179-b051-4c2c-a684-ef3a4fb0f80f",
                        "name": "Maintenance Window",
                        "description": "planned downtime",
                        "metaLabels": [],
                        "filter": {
                            "whatExpression": "source logs | filter true",
                            "alertUniqueIds": {"value": []}
                        },
                        "schedule": {
                            "scheduleOperation": "SCHEDULE_OPERATION_MUTE",
                            "oneTime": {
                                "timeframe": {
                                    "startTime": "2026-08-10T19:16:45.000",
                                    "endTime": "2026-08-10T20:16:45.000",
                                    "timezone": "UTC"
                                }
                            }
                        },
                        "enabled": true,
                        "createdAt": "2026-08-10T18:17:04.000Z",
                        "updatedAt": "2026-08-10T18:17:04.000Z"
                    },
                    "nextActiveTimeframes": []
                }
            ]
        })
    }

    #[test]
    fn deserialize_list_response() {
        let resp: GetBulkAlertSchedulerRuleResponse =
            serde_json::from_value(list_response_fixture()).unwrap();
        assert_eq!(resp.alert_scheduler_rules.len(), 1);

        let rule = resp.alert_scheduler_rules[0]
            .alert_scheduler_rule
            .as_ref()
            .expect("list entry should carry a rule");
        assert_eq!(rule.name.as_deref(), Some("Maintenance Window"));
        assert_eq!(rule.description.as_deref(), Some("planned downtime"));
        assert_eq!(rule.enabled, Some(true));
        assert_eq!(rule.created_at.as_deref(), Some("2026-08-10T18:17:04.000Z"));
    }

    #[test]
    fn list_response_keeps_the_two_ids_apart() {
        let resp: GetBulkAlertSchedulerRuleResponse =
            serde_json::from_value(list_response_fixture()).unwrap();
        let rule = resp.alert_scheduler_rules[0]
            .alert_scheduler_rule
            .as_ref()
            .unwrap();

        assert_eq!(
            rule.unique_identifier.as_deref(),
            Some("38c4a964-a237-41ea-9b02-87af3d734571")
        );
        assert_eq!(
            rule.id.as_deref(),
            Some("04b68179-b051-4c2c-a684-ef3a4fb0f80f")
        );
        assert_ne!(rule.unique_identifier, rule.id);
    }

    /// Treating entries as bare rules silently yielded all-null fields.
    #[test]
    fn list_entries_are_not_bare_rules() {
        let resp: GetBulkAlertSchedulerRuleResponse =
            serde_json::from_value(list_response_fixture()).unwrap();
        let entry = &resp.alert_scheduler_rules[0];
        assert!(entry.alert_scheduler_rule.is_some());
        assert!(entry.next_active_timeframes.is_empty());
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
                "uniqueIdentifier": "38c4a964-a237-41ea-9b02-87af3d734571",
                "id": "04b68179-b051-4c2c-a684-ef3a4fb0f80f",
                "name": "New Rule"
            }
        });
        let resp: CreateAlertSchedulerRuleResponse = serde_json::from_value(json).unwrap();
        let rule = resp.alert_scheduler_rule.unwrap();
        assert_eq!(
            rule.unique_identifier.as_deref(),
            Some("38c4a964-a237-41ea-9b02-87af3d734571")
        );
        assert_eq!(
            rule.id.as_deref(),
            Some("04b68179-b051-4c2c-a684-ef3a4fb0f80f")
        );
        assert_eq!(rule.name.as_deref(), Some("New Rule"));
    }

    #[test]
    fn deserialize_update_response_keeps_unique_identifier_stable() {
        let json = json!({
            "alertSchedulerRule": {
                "uniqueIdentifier": "50fb552b-c042-4a8c-8186-fd488d452fc9",
                "id": "808f396d-4e85-4872-b18f-9b1a39f466a5",
                "name": "Updated Rule",
                "enabled": false
            }
        });
        let resp: UpdateAlertSchedulerRuleResponse = serde_json::from_value(json).unwrap();
        let rule = resp.alert_scheduler_rule.unwrap();
        assert_eq!(
            rule.unique_identifier.as_deref(),
            Some("50fb552b-c042-4a8c-8186-fd488d452fc9")
        );
        assert_ne!(rule.unique_identifier, rule.id);
    }

    #[test]
    fn rule_found_accepts_a_populated_get_response() {
        let json = json!({
            "alertSchedulerRule": {
                "uniqueIdentifier": "38c4a964-a237-41ea-9b02-87af3d734571",
                "name": "Maintenance Window"
            }
        });
        assert!(rule_found(&json));
    }

    #[test]
    fn rule_found_rejects_an_empty_get_response() {
        assert!(!rule_found(&json!({})));
        assert!(!rule_found(&json!({"alertSchedulerRule": {}})));
        assert!(!rule_found(&json!({"alertSchedulerRule": null})));
    }

    #[test]
    fn classify_recognises_a_unique_identifier_as_addressable() {
        let resp: GetBulkAlertSchedulerRuleResponse =
            serde_json::from_value(list_response_fixture()).unwrap();
        assert_eq!(
            classify_rule_id_in(&resp, "38c4a964-a237-41ea-9b02-87af3d734571"),
            RuleIdKind::Addressable
        );
    }

    #[test]
    fn classify_maps_a_version_id_to_its_unique_identifier() {
        let resp: GetBulkAlertSchedulerRuleResponse =
            serde_json::from_value(list_response_fixture()).unwrap();
        assert_eq!(
            classify_rule_id_in(&resp, "04b68179-b051-4c2c-a684-ef3a4fb0f80f"),
            RuleIdKind::VersionId("38c4a964-a237-41ea-9b02-87af3d734571".to_string())
        );
    }

    #[test]
    fn classify_reports_an_unknown_id() {
        let resp: GetBulkAlertSchedulerRuleResponse =
            serde_json::from_value(list_response_fixture()).unwrap();
        assert_eq!(
            classify_rule_id_in(&resp, "ffffffff-ffff-ffff-ffff-ffffffffffff"),
            RuleIdKind::Unknown
        );
    }
}
