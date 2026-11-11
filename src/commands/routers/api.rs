use serde::Deserialize;
use serde_json::{json, Value};

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

/// Turn the matcher definition into the query parameters the API expects.
/// The file may hold the labels flat (`{"routing.environment": "prod"}`) or
/// wrapped as `{"entityLabelMatcher": {...}}` / `{"entity_label_matcher":
/// {...}}`. The API only accepts the keys `routing.team`, `routing.service`
/// and `routing.environment`; it reports any other key in the result.
pub fn matcher_query_params(body: &Value) -> Vec<(String, String)> {
    let labels = body
        .get("entityLabelMatcher")
        .or_else(|| body.get("entity_label_matcher"))
        .unwrap_or(body);
    labels
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    let value = match value {
                        Value::String(s) => s.clone(),
                        Value::Null => return None,
                        other => other.to_string(),
                    };
                    Some((format!("entity_label_matcher[{key}]"), value))
                })
                .collect()
        })
        .unwrap_or_default()
}

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

    /// The API reads the matcher from `entity_label_matcher[<key>]=<value>`
    /// query parameters and ignores the request body (a body-only call always
    /// answers "Routing labels cannot be empty"). See [`matcher_query_params`].
    pub async fn validate_matcher(&self, body: &Value) -> Result<Value> {
        let path = format!("{ROUTERS_BASE}/matcher/validate");
        let params = matcher_query_params(body);
        let params: Vec<(&str, &str)> = params
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        self.client
            .post_with_headers(&path, Some(&params), &json!({}), &[])
            .await
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

    #[test]
    fn matcher_params_accept_flat_and_wrapped_labels() {
        let flat = json!({ "routing.environment": "prod" });
        assert_eq!(
            matcher_query_params(&flat),
            vec![(
                "entity_label_matcher[routing.environment]".to_string(),
                "prod".to_string()
            )]
        );
        let wrapped = json!({ "entityLabelMatcher": { "routing.team": "cx", "skip": null } });
        assert_eq!(
            matcher_query_params(&wrapped),
            vec![(
                "entity_label_matcher[routing.team]".to_string(),
                "cx".to_string()
            )]
        );
        assert!(matcher_query_params(&json!("not an object")).is_empty());
    }
}
