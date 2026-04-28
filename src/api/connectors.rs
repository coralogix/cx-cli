use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;
use super::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub connector_type: Option<String>,
    pub enabled: Option<bool>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
}

impl Connector {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_type(&self) -> String {
        self.connector_type
            .as_deref()
            .map(|s| s.strip_prefix("CONNECTOR_TYPE_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListConnectorsResponse {
    #[serde(default)]
    pub connectors: Vec<Connector>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConnectorResponse {
    pub connector: Option<Connector>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConnectorResponse {
    pub connector: Option<Connector>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteConnectorResponse {}

// --- API ---

const CONNECTORS_BASE: &str = "/mgmt/openapi/5/notifications/notification-center/v1/connectors";

pub struct ConnectorsApi<'a> {
    client: &'a CxClient,
}

impl<'a> ConnectorsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListConnectorsResponse> {
        self.client.get(CONNECTORS_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{CONNECTORS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateConnectorResponse> {
        self.client.post(CONNECTORS_BASE, body).await
    }

    pub async fn replace(&self, body: &Value) -> Result<Value> {
        self.client.put(CONNECTORS_BASE, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteConnectorResponse> {
        let path = format!("{CONNECTORS_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn get_type_summaries(&self) -> Result<Value> {
        let path = format!("{CONNECTORS_BASE}/types/summaries");
        self.client.get(&path, &[]).await
    }

    pub async fn list_entity_types(&self) -> Result<Value> {
        let path = format!("{CONNECTORS_BASE}/entity-types");
        self.client.get(&path, &[]).await
    }

    pub async fn list_entity_subtypes(&self, entity_type: &str) -> Result<Value> {
        let path = format!("{CONNECTORS_BASE}/entity-types/{entity_type}/subtypes");
        self.client.get(&path, &[]).await
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
            "connectors": [
                {
                    "id": "conn-001",
                    "name": "Slack Notifications",
                    "type": "CONNECTOR_TYPE_SLACK",
                    "enabled": true,
                    "createTime": "2024-01-01T00:00:00Z"
                },
                {
                    "id": "conn-002",
                    "name": "PagerDuty",
                    "type": "CONNECTOR_TYPE_PAGERDUTY",
                    "enabled": false
                }
            ]
        });

        let resp: ListConnectorsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.connectors.len(), 2);
        assert_eq!(resp.connectors[0].display_name(), "Slack Notifications");
        assert_eq!(resp.connectors[0].display_type(), "SLACK");
        assert_eq!(resp.connectors[0].enabled, Some(true));
        assert_eq!(resp.connectors[1].display_type(), "PAGERDUTY");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListConnectorsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.connectors.is_empty());
    }

    #[test]
    fn deserialize_get_response() {
        let json = json!({
            "connector": {
                "id": "conn-001",
                "name": "Slack Notifications"
            }
        });
        let resp: GetConnectorResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.connector.unwrap().id.as_deref(), Some("conn-001"));
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "connector": { "id": "conn-new", "name": "New Connector" }
        });
        let resp: CreateConnectorResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.connector.unwrap().id.as_deref(), Some("conn-new"));
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteConnectorResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn deserialize_entity_types_empty() {
        let json = json!({});
        let _resp: Value = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn display_missing_fields() {
        let conn = Connector {
            id: None,
            name: None,
            connector_type: None,
            enabled: None,
            create_time: None,
            update_time: None,
        };
        assert_eq!(conn.display_name(), "-");
        assert_eq!(conn.display_type(), "-");
    }
}
