use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;
use super::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub action_type: Option<String>,
    pub url: Option<String>,
    pub is_active: Option<bool>,
}

impl Action {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_type(&self) -> &str {
        self.action_type.as_deref().unwrap_or("-")
    }

    pub fn display_url(&self) -> &str {
        self.url.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListActionsResponse {
    #[serde(default)]
    pub actions: Vec<Action>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateActionResponse {
    pub action: Option<Action>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteActionResponse {}

// --- API ---

const ACTIONS_BASE: &str = "/mgmt/openapi/5/actions/actions/v2";
const ACTIONS_BATCH: &str = "/mgmt/openapi/5/actions/batch/v2";
const ACTIONS_ORDER: &str = "/mgmt/openapi/5/actions/order/v2";

pub struct ActionsApi<'a> {
    client: &'a CxClient,
}

impl<'a> ActionsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListActionsResponse> {
        self.client.get(ACTIONS_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{ACTIONS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateActionResponse> {
        self.client.post(ACTIONS_BASE, body).await
    }

    pub async fn replace(&self, id: &str, body: &Value) -> Result<Value> {
        let path = format!("{ACTIONS_BASE}/{id}");
        self.client.put(&path, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteActionResponse> {
        let path = format!("{ACTIONS_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn batch_execute(&self, body: &Value) -> Result<Value> {
        self.client.post(ACTIONS_BATCH, body).await
    }

    pub async fn order(&self, body: &Value) -> Result<Value> {
        self.client.post(ACTIONS_ORDER, body).await
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
            "actions": [
                {
                    "id": "action-001",
                    "name": "Send to Slack",
                    "type": "slack",
                    "url": "https://hooks.slack.com/services/xxx",
                    "isActive": true
                },
                {
                    "id": "action-002",
                    "name": "PagerDuty Alert",
                    "type": "pagerduty",
                    "url": "https://events.pagerduty.com/v2/enqueue",
                    "isActive": false
                }
            ]
        });

        let resp: ListActionsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.actions.len(), 2);
        assert_eq!(resp.actions[0].display_name(), "Send to Slack");
        assert_eq!(resp.actions[0].display_type(), "slack");
        assert_eq!(
            resp.actions[0].display_url(),
            "https://hooks.slack.com/services/xxx"
        );
        assert_eq!(resp.actions[0].is_active, Some(true));
        assert_eq!(resp.actions[1].display_type(), "pagerduty");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListActionsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.actions.is_empty());
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "action": { "id": "action-new", "name": "New Action" }
        });
        let resp: CreateActionResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.action.unwrap().id.as_deref(), Some("action-new"));
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteActionResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn display_missing_fields() {
        let action = Action {
            id: None,
            name: None,
            action_type: None,
            url: None,
            is_active: None,
        };
        assert_eq!(action.display_name(), "-");
        assert_eq!(action.display_type(), "-");
        assert_eq!(action.display_url(), "-");
    }
}
