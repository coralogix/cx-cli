use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;
use super::serde_helpers::string_or_number;

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
    pub webhooks: Vec<Webhook>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookResponse {
    pub webhook: Option<Webhook>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteWebhookResponse {}

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

    pub async fn create(&self, body: &Value) -> Result<CreateWebhookResponse> {
        self.client.post(WEBHOOKS_BASE, body).await
    }

    pub async fn update(&self, id: &str, body: &Value) -> Result<Value> {
        let path = format!("{WEBHOOKS_BASE}/{id}");
        self.client.put(&path, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteWebhookResponse> {
        let path = format!("{WEBHOOKS_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn test(&self, id: &str) -> Result<Value> {
        let path = format!("{WEBHOOKS_BASE}/{id}/test");
        self.client.post(&path, &serde_json::json!({})).await
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
            "webhooks": [
                { "id": "wh-001", "name": "Slack Notify", "type": "slack", "url": "https://hooks.slack.com/..." }
            ]
        });
        let resp: ListWebhooksResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.webhooks.len(), 1);
        assert_eq!(resp.webhooks[0].display_name(), "Slack Notify");
        assert_eq!(resp.webhooks[0].display_type(), "slack");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({ "webhooks": [] });
        let resp: ListWebhooksResponse = serde_json::from_value(json).unwrap();
        assert!(resp.webhooks.is_empty());
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({ "webhook": { "id": "wh-001", "name": "Slack Notify" } });
        let resp: CreateWebhookResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.webhook.unwrap().id.as_deref(), Some("wh-001"));
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
