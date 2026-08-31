use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Response types ---

/// A suppression rule, as returned by the alert-scheduler API.
///
/// Note the two distinct IDs. Per `alert_scheduler_rule.proto`, `id` is the
/// rule *version* id (it changes on every update) while `unique_identifier`
/// is the rule's own stable id. `unique_identifier` is the one every route
/// takes: `GET`/`DELETE .../v1/{id}`, the `uniqueIdentifier` key in a `PUT`
/// body, and the console's `?edit=` parameter (the suppression-rules page
/// resolves it via `rules.find(r => r?.uniqueIdentifier === id)`). Passing
/// the version id instead is not an error - `GET` returns `{}` and `DELETE`
/// returns 200 without deleting anything - so keep the two apart.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertSchedulerRule {
    /// The rule's stable id. Use this for get/update/delete and console links.
    pub unique_identifier: Option<String>,
    /// The rule *version* id - changes on every update. Not addressable.
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub schedule: Option<Value>,
    pub meta_labels: Option<Vec<Value>>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// One entry in a list response. The list endpoint wraps each rule in its own
/// object alongside the rule's upcoming activation windows, so items are
/// `{"alertSchedulerRule": {...}, "nextActiveTimeframes": [...]}` rather than
/// bare rules.
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

    /// Fetch one rule by its `unique_identifier`.
    ///
    /// The endpoint answers 200 with an empty object `{}` for an id it doesn't
    /// know - including a valid-looking rule *version* id - rather than 404.
    /// Use [`rule_found`] on the result instead of relying on the status code.
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

    /// Delete one rule by its `unique_identifier`.
    ///
    /// Answers 200 with `{}` whether or not anything was deleted, so callers
    /// must confirm the rule exists via [`AlertSchedulersApi::get`] first or a
    /// no-op reads as success.
    pub async fn delete(&self, id: &str) -> Result<DeleteAlertSchedulerRuleResponse> {
        let path = format!("{SCHEDULERS_BASE}/{id}");
        self.client.delete(&path).await
    }
}

/// Whether a [`AlertSchedulersApi::get`] response actually carries a rule.
///
/// The endpoint returns `{}` rather than a 404 for an unknown id, so an
/// `Ok(value)` on its own says nothing about whether the rule exists.
pub fn rule_found(val: &Value) -> bool {
    val.get("alertSchedulerRule")
        .is_some_and(|r| r.is_object() && r.as_object().is_some_and(|m| !m.is_empty()))
}

/// How an input id relates to the rules that actually exist.
#[derive(Debug, PartialEq)]
pub enum RuleIdKind {
    /// Matches a rule's `unique_identifier` - addressable as-is.
    Addressable,
    /// Matches a rule's *version* `id`; carries the addressable `unique_identifier`.
    VersionId(String),
    /// Matches no rule under either field.
    Unknown,
}

/// Classify an id against an already-fetched rule list.
///
/// `Addressable` wins over `VersionId` if (pathologically) an id matches both a
/// rule's `unique_identifier` and another rule's version `id`, since the
/// addressable interpretation is the one every route accepts.
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

/// Classify `input` against the live rule set. Costs one `list` call.
///
/// Because `get`/`delete` answer 200 for an unknown or version id rather than
/// erroring, listing every rule and matching on both id fields is the only way
/// to tell an addressable `unique_identifier` from a rule *version* id.
pub async fn classify_rule_id(api: &AlertSchedulersApi<'_>, input: &str) -> Result<RuleIdKind> {
    Ok(classify_rule_id_in(&api.list().await?, input))
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The list endpoint's real shape: rules are wrapped one level deeper than
    /// the envelope key suggests, and each carries both IDs. Captured from a
    /// live `GET /mgmt/openapi/5/alerts/suppression-rules/v1`.
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

    /// The regression this module exists for: a rule's two IDs are different
    /// values, and the addressable one is `uniqueIdentifier`. Modelling only
    /// `id` produced console links that never resolved and deletes that
    /// silently did nothing.
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

    /// Regression guard for the bug that made every listed field render as
    /// null: the old model expected `alertSchedulerRules` to hold bare rules,
    /// so serde matched nothing and quietly produced `None` everywhere while
    /// still exiting 0.
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

    /// An update mints a fresh version id while `uniqueIdentifier` stays put -
    /// which is exactly why the console link has to be built from the latter.
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

    /// The miss case: an unknown id (or a version id) answers 200 `{}`, not a
    /// 404, so the status code alone can't tell a hit from a miss.
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

    /// The point of the feature: a version id resolves to the rule's addressable
    /// `unique_identifier` so callers can auto-correct.
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
