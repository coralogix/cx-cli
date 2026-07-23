use std::collections::HashMap;

use serde::Deserialize;

use crate::api_client::CxClient;
use crate::error::Result;

// ── Response types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAvailableResourceTypesResponse {
    #[serde(default)]
    pub resource_types: Vec<ResourceTypeMapping>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTypeMapping {
    pub category_type: Option<CategoryType>,
    pub resource_type: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CategoryType {
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResourcesResponse {
    #[serde(default)]
    pub resources: Vec<ResourceData>,
    pub total_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceData {
    pub resource_id: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub columns: HashMap<String, String>,
}

/// Query parameters for [`InfraApi::list`]. Scope filters are sent as flat
/// `scopeFilter.{key}` query parameters, matching the API contract.
pub struct ListResourcesParams<'p> {
    pub category: &'p str,
    pub resource_type: &'p str,
    pub name_filter: Option<&'p str>,
    pub scope_filters: &'p [(String, String)],
    pub start_row: Option<i64>,
    pub end_row: Option<i64>,
}

// ── API ────────────────────────────────────────────────────────────────────────

const BASE_PATH: &str = "/infrastructure/resources/v1";

pub struct InfraApi<'a> {
    client: &'a CxClient,
}

impl<'a> InfraApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// List the available resource type mappings (category/type pairs).
    pub async fn available_types(&self) -> Result<GetAvailableResourceTypesResponse> {
        let path = format!("{BASE_PATH}/types");
        self.client.get(&path, &[]).await
    }

    /// List resources of a given category and type, with optional name filter,
    /// scope filters, and a `startRow`/`endRow` page window.
    pub async fn list(&self, params: &ListResourcesParams<'_>) -> Result<GetResourcesResponse> {
        let mut query: Vec<(String, String)> = vec![
            ("category".to_string(), params.category.to_string()),
            ("type".to_string(), params.resource_type.to_string()),
        ];
        if let Some(name) = params.name_filter {
            query.push(("nameFilter".to_string(), name.to_string()));
        }
        for (key, value) in params.scope_filters {
            query.push((format!("scopeFilter.{key}"), value.clone()));
        }
        if let Some(start) = params.start_row {
            query.push(("startRow".to_string(), start.to_string()));
        }
        if let Some(end) = params.end_row {
            query.push(("endRow".to_string(), end.to_string()));
        }
        let query_refs: Vec<(&str, &str)> = query
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        self.client.get(BASE_PATH, &query_refs).await
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_available_types_response() {
        let json = json!({
            "resourceTypes": [
                {
                    "categoryType": { "category": "Hosts", "type": "EC2_Instances" },
                    "resourceType": "aws_ec2_instance",
                    "label": "EC2 Instances"
                },
                {
                    "categoryType": { "category": "Hosts", "type": "Azure_VMs" },
                    "resourceType": "azure_vm",
                    "label": "Azure Virtual Machines"
                }
            ]
        });

        let resp: GetAvailableResourceTypesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.resource_types.len(), 2);
        let first = &resp.resource_types[0];
        let category_type = first.category_type.as_ref().unwrap();
        assert_eq!(category_type.category.as_deref(), Some("Hosts"));
        assert_eq!(category_type.type_name.as_deref(), Some("EC2_Instances"));
        assert_eq!(first.resource_type.as_deref(), Some("aws_ec2_instance"));
        assert_eq!(first.label.as_deref(), Some("EC2 Instances"));
    }

    #[test]
    fn deserialize_empty_types_response() {
        let json = json!({});
        let resp: GetAvailableResourceTypesResponse = serde_json::from_value(json).unwrap();
        assert!(resp.resource_types.is_empty());
    }

    #[test]
    fn deserialize_types_response_with_missing_fields() {
        let json = json!({
            "resourceTypes": [
                { "resourceType": "aws_ec2_instance" }
            ]
        });
        let resp: GetAvailableResourceTypesResponse = serde_json::from_value(json).unwrap();
        let first = &resp.resource_types[0];
        assert!(first.category_type.is_none());
        assert!(first.label.is_none());
    }

    #[test]
    fn deserialize_resources_response() {
        let json = json!({
            "resources": [
                {
                    "resourceId": "1001234:host_id=i-abc123",
                    "name": "web-server-1",
                    "columns": { "region": "us-east-1", "instance_type": "m5.large" }
                },
                {
                    "resourceId": "1001234:host_id=i-def456",
                    "name": "web-server-2",
                    "columns": {}
                }
            ],
            "totalCount": 42
        });

        let resp: GetResourcesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.resources.len(), 2);
        assert_eq!(resp.total_count, Some(42));
        let first = &resp.resources[0];
        assert_eq!(
            first.resource_id.as_deref(),
            Some("1001234:host_id=i-abc123")
        );
        assert_eq!(first.name.as_deref(), Some("web-server-1"));
        assert_eq!(
            first.columns.get("region").map(String::as_str),
            Some("us-east-1")
        );
        assert!(resp.resources[1].columns.is_empty());
    }

    #[test]
    fn deserialize_empty_resources_response() {
        let json = json!({});
        let resp: GetResourcesResponse = serde_json::from_value(json).unwrap();
        assert!(resp.resources.is_empty());
        assert_eq!(resp.total_count, None);
    }

    #[test]
    fn deserialize_resource_with_missing_columns() {
        let json = json!({
            "resources": [{ "resourceId": "1001234:host_id=i-abc123", "name": "web-server-1" }],
            "totalCount": 1
        });
        let resp: GetResourcesResponse = serde_json::from_value(json).unwrap();
        assert!(resp.resources[0].columns.is_empty());
    }
}
