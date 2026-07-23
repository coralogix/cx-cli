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
}
