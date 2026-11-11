use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::Result;

use crate::api_client::CxClient;
use crate::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Webhook {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub webhook_type: Option<String>,
    pub url: Option<String>,
    pub created_at: Option<String>,
}

impl Webhook {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_type(&self) -> &str {
        self.webhook_type.as_deref().unwrap_or("-")
    }

    pub fn display_url(&self) -> &str {
        self.url.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListWebhooksResponse {
    #[serde(default)]
    pub deployed: Vec<Webhook>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteWebhookResponse {}

/// Build the replace request. A file that already has the `data` envelope is
/// kept; a file holding only the inner webhook data is wrapped. The id from
/// the command line always wins over one in the file, so `cx webhooks update
/// <id>` edits the webhook the user named.
pub fn update_request_body(id: &str, body: &Value) -> Value {
    let mut out = if body.get("data").is_some() {
        body.clone()
    } else {
        json!({ "data": body })
    };
    out["id"] = json!(id);
    out
}

// --- API ---

const WEBHOOKS_BASE: &str = "/mgmt/openapi/5/integrations/webhooks/v1";
const WEBHOOK_TYPES_BASE: &str = "/mgmt/openapi/5/integrations/webhook-types/v1";

pub struct WebhooksApi<'a> {
    client: &'a CxClient,
}

impl<'a> WebhooksApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list_all(&self) -> Result<ListWebhooksResponse> {
        self.client.get(WEBHOOKS_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{WEBHOOKS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    /// Returns the raw response, like `get`/`update` do. The API has been
    /// observed to return the created webhook bare (`{"id": "..."}`) rather
    /// than wrapped in a `{"webhook": {...}}` envelope, so callers must not
    /// deserialize into a shape that assumes one specific wrapper.
    pub async fn create(&self, body: &Value) -> Result<Value> {
        self.client.post(WEBHOOKS_BASE, body).await
    }

    /// The API replaces a webhook with `PUT /webhooks/v1` and the id in the
    /// body (`PUT /webhooks/v1/{id}` is a 405). See [`update_request_body`].
    pub async fn update(&self, id: &str, body: &Value) -> Result<Value> {
        self.client
            .put(WEBHOOKS_BASE, &update_request_body(id, body))
            .await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteWebhookResponse> {
        let path = format!("{WEBHOOKS_BASE}/{id}");
        self.client.delete(&path).await
    }

    /// Testing an existing webhook is a `GET` (a `POST` is a 405). The
    /// response carries `{"success": {}}` or `{"failure": {...}}` with status
    /// 200 either way; a delivery failure is data, not an error.
    pub async fn test(&self, id: &str) -> Result<Value> {
        let path = format!("{WEBHOOKS_BASE}/{id}/test");
        self.client.get(&path, &[]).await
    }

    pub async fn list_types(&self) -> Result<Value> {
        self.client.get(WEBHOOK_TYPES_BASE, &[]).await
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
            "deployed": [
                { "id": "wh-001", "name": "Slack Notify", "type": "slack", "url": "https://hooks.slack.com/..." }
            ]
        });
        let resp: ListWebhooksResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.deployed.len(), 1);
        assert_eq!(resp.deployed[0].display_name(), "Slack Notify");
        assert_eq!(resp.deployed[0].display_type(), "slack");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({ "deployed": [] });
        let resp: ListWebhooksResponse = serde_json::from_value(json).unwrap();
        assert!(resp.deployed.is_empty());
    }

    #[test]
    fn update_body_wraps_bare_data_and_sets_id() {
        let bare = json!({ "name": "n", "type": "GENERIC", "url": "https://example.com" });
        let out = update_request_body("wh-1", &bare);
        assert_eq!(out["id"], "wh-1");
        assert_eq!(out["data"], bare);
    }

    #[test]
    fn update_body_keeps_envelope_and_overrides_id() {
        let wrapped = json!({ "id": "stale", "data": { "name": "n" } });
        let out = update_request_body("wh-2", &wrapped);
        assert_eq!(out["id"], "wh-2");
        assert_eq!(out["data"]["name"], "n");
    }

    #[test]
    fn display_missing_fields() {
        let w = Webhook {
            id: None,
            name: None,
            webhook_type: None,
            url: None,
            created_at: None,
        };
        assert_eq!(w.display_name(), "-");
        assert_eq!(w.display_type(), "-");
        assert_eq!(w.display_url(), "-");
    }
}
