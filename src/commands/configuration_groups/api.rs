use serde::Deserialize;
use serde_json::Value;

use crate::api_client::CxClient;
use crate::error::Result;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListConfigurationGroupsResponse {
    #[serde(default)]
    pub groups: Vec<Value>,
    pub next_page_token: Option<String>,
}

const GROUPS_BASE: &str = "/mgmt/openapi/5/fleet-management/configuration-groups/v1";

/// Build the supported query parameters for one group-list page.
pub fn list_query_params(
    include_archived: bool,
    page_size: Option<u16>,
    page_token: Option<&str>,
) -> Vec<(String, String)> {
    let mut params = Vec::new();
    if include_archived {
        params.push(("include_archived".to_string(), "true".to_string()));
    }
    if let Some(page_size) = page_size {
        params.push(("page_size".to_string(), page_size.to_string()));
    }
    if let Some(page_token) = page_token {
        params.push(("page_token".to_string(), page_token.to_string()));
    }
    params
}

pub struct ConfigurationGroupsApi<'a> {
    client: &'a CxClient,
}

impl<'a> ConfigurationGroupsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(
        &self,
        include_archived: bool,
        page_size: Option<u16>,
        page_token: Option<&str>,
    ) -> Result<ListConfigurationGroupsResponse> {
        let owned = list_query_params(include_archived, page_size, page_token);
        let params: Vec<(&str, &str)> = owned
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        self.client.get(GROUPS_BASE, &params).await
    }

    pub async fn get(&self, id: &str, include_archived: bool) -> Result<Value> {
        let path = format!("{GROUPS_BASE}/{id}");
        let params = if include_archived {
            vec![("include_archived", "true")]
        } else {
            Vec::new()
        };
        self.client.get(&path, &params).await
    }

    pub async fn create(&self, body: &Value) -> Result<Value> {
        self.client.post(GROUPS_BASE, body).await
    }

    pub async fn update(&self, id: &str, update_mask: &str, body: &Value) -> Result<Value> {
        let path = format!("{GROUPS_BASE}/{id}");
        self.client
            .patch_with_query(&path, &[("update_mask", update_mask)], body)
            .await
    }

    pub async fn archive(&self, id: &str) -> Result<Value> {
        let path = format!("{GROUPS_BASE}/{id}/archive");
        self.client.post_empty(&path, &[]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn list_query_includes_requested_filters_and_pagination() {
        assert_eq!(
            list_query_params(true, Some(50), Some("opaque_cursor-1")),
            vec![
                ("include_archived".to_string(), "true".to_string()),
                ("page_size".to_string(), "50".to_string()),
                ("page_token".to_string(), "opaque_cursor-1".to_string()),
            ]
        );
        assert!(list_query_params(false, None, None).is_empty());
    }

    #[test]
    fn list_response_preserves_nested_groups_and_next_page_token() {
        let response: ListConfigurationGroupsResponse = serde_json::from_value(json!({
            "groups": [{
                "id": "group-1",
                "family": {"active": false, "raw": {"remoteConfigurations": []}}
            }],
            "nextPageToken": "next_page"
        }))
        .unwrap();

        assert_eq!(response.groups.len(), 1);
        assert_eq!(response.groups[0]["family"]["active"], false);
        assert_eq!(response.next_page_token.as_deref(), Some("next_page"));
    }

    #[test]
    fn list_response_accepts_missing_groups_and_page_token() {
        let response: ListConfigurationGroupsResponse = serde_json::from_value(json!({})).unwrap();

        assert!(response.groups.is_empty());
        assert_eq!(response.next_page_token, None);
    }
}
