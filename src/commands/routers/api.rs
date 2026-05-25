use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;
use crate::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Router {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    pub entity_type: Option<String>,
    pub destinations: Option<Vec<Value>>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
}

impl Router {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_entity_type(&self) -> String {
        self.entity_type
            .as_deref()
            .map(|s| s.strip_prefix("ENTITY_TYPE_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn destinations_count(&self) -> usize {
        self.destinations.as_ref().map(|d| d.len()).unwrap_or(0)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRoutersResponse {
    #[serde(default)]
    pub routers: Vec<Router>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRouterResponse {
    pub router: Option<Router>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRouterResponse {
    pub router: Option<Router>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteRouterResponse {}

// --- API ---

const ROUTERS_BASE: &str = "/mgmt/openapi/5/notifications/notification-center/v1/routers";

pub struct RoutersApi<'a> {
    client: &'a CxClient,
}

impl<'a> RoutersApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListRoutersResponse> {
        self.client.get(ROUTERS_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{ROUTERS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateRouterResponse> {
        self.client.post(ROUTERS_BASE, body).await
    }

    pub async fn replace(&self, body: &Value) -> Result<Value> {
        self.client.put(ROUTERS_BASE, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteRouterResponse> {
        let path = format!("{ROUTERS_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn validate_matcher(&self, body: &Value) -> Result<Value> {
        let path = format!("{ROUTERS_BASE}/matcher/validate");
        self.client.post(&path, body).await
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
            "routers": [
                {
                    "id": "router-001",
                    "name": "Alert Router",
                    "entityType": "ENTITY_TYPE_ALERTS",
                    "destinations": [{"id": "dest-1"}, {"id": "dest-2"}],
                    "createTime": "2024-01-01T00:00:00Z"
                }
            ]
        });

        let resp: ListRoutersResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.routers.len(), 1);
        assert_eq!(resp.routers[0].display_name(), "Alert Router");
        assert_eq!(resp.routers[0].display_entity_type(), "ALERTS");
        assert_eq!(resp.routers[0].destinations_count(), 2);
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListRoutersResponse = serde_json::from_value(json).unwrap();
        assert!(resp.routers.is_empty());
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteRouterResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn display_missing_fields() {
        let router = Router {
            id: None,
            name: None,
            entity_type: None,
            destinations: None,
            create_time: None,
            update_time: None,
        };
        assert_eq!(router.display_name(), "-");
        assert_eq!(router.display_entity_type(), "-");
        assert_eq!(router.destinations_count(), 0);
    }
}
