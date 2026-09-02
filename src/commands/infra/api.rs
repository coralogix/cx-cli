use std::collections::BTreeMap;

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api_client::CxClient;
use crate::error::Result;

const BASE_PATH: &str = "/mgmt/api/infrastructure/resources/v1";

// ── Response types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAvailableResourceTypesResponse {
    #[serde(default)]
    pub resource_types: Vec<ResourceTypeMapping>,
}

/// The API also returns `resourceType`, which the CLI deliberately does not
/// surface in any output format. This is intentional and will be added later on
/// when the CLI will offer other commands that support it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTypeMapping {
    pub category_type: Option<CategoryType>,
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
pub struct GetFiltersResponse {
    #[serde(default)]
    pub filters: Vec<FilterDescriptor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterDescriptor {
    pub name: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub wildcard: bool,
    /// Absent for an open set; present only for a closed one such as `Health`.
    #[serde(default)]
    pub values: Vec<String>,
    /// Absent when the request pinned a type.
    #[serde(default)]
    pub types: Vec<CategoryType>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResourcesResponse {
    #[serde(default)]
    pub resources: Vec<ResourceData>,
    pub total_count: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceData {
    pub resource_id: Option<String>,
    pub name: Option<String>,
    /// `BTreeMap` so that `serde_json` maintains column order.
    #[serde(default)]
    pub columns: BTreeMap<String, String>,
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetHealthHistoryResponse {
    #[serde(default)]
    pub health_history: Vec<HealthHistoryEntry>,
}

#[derive(Debug, Deserialize)]
pub struct HealthHistoryEntry {
    /// RFC 3339 timestamp of the daily sample.
    pub timestamp: Option<String>,
    /// `Healthy`, `Critical`, or `Unmonitored`.
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRawDataResponse {
    /// The raw resource document; `null` when the document is cleanly missing.
    pub raw_data: Option<Value>,
}

/// Scope filters travel as flat `scopeFilter.{key}` query parameters whichever
/// method is used; the rest moves into the body when a filter is present.
pub struct ListResourcesParams<'p> {
    pub category: Option<&'p str>,
    pub resource_type: Option<&'p str>,
    pub name_filter: Option<&'p str>,
    pub scope_filters: &'p [(String, String)],
    pub filter: Option<&'p Filter>,
    pub start_row: Option<i64>,
    pub end_row: Option<i64>,
}

/// One node of the filter the API accepts: a `match` on a single attribute, or a
/// `bool` joining operands.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Filter {
    Match(FieldMatch),
    Bool(BoolFilter),
}

/// The values of one attribute are alternatives.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldMatch {
    pub field: String,
    pub values: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BoolFilter {
    pub op: Op,
    pub operands: Vec<Filter>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Op {
    And,
    Or,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListResourcesBody<'p> {
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<&'p str>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    resource_type: Option<&'p str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name_filter: Option<&'p str>,
    filter: &'p Filter,
}

// ── API ────────────────────────────────────────────────────────────────────────

/// Resource ids embed reserved characters (`:`, `|`, `=`) and travel as a URL
/// path segment, so encode everything except RFC 3986 unreserved characters.
const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Percent-encodes a resource id for use as a URL path segment, so users can
/// pass ids exactly as returned by `list`.
pub fn encode_resource_id(resource_id: &str) -> String {
    utf8_percent_encode(resource_id, PATH_SEGMENT_ENCODE_SET).to_string()
}

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

    /// List the filterable attributes. Narrowest first: both parameters pin one
    /// type, a category alone spans its types, neither spans everything.
    pub async fn filters(
        &self,
        category: Option<&str>,
        resource_type: Option<&str>,
    ) -> Result<GetFiltersResponse> {
        let mut query: Vec<(&str, &str)> = Vec::new();
        if let Some(category) = category {
            query.push(("category", category));
        }
        if let Some(resource_type) = resource_type {
            query.push(("type", resource_type));
        }
        let path = format!("{BASE_PATH}/filters");
        self.client.get(&path, &query).await
    }

    /// A filter has to travel in a body, so a filtered request goes over `POST`
    /// and an unfiltered one keeps using `GET`.
    pub async fn list(&self, params: &ListResourcesParams<'_>) -> Result<GetResourcesResponse> {
        let mut query: Vec<(String, String)> = Vec::new();
        if params.filter.is_none() {
            if let Some(category) = params.category {
                query.push(("category".to_string(), category.to_string()));
            }
            if let Some(resource_type) = params.resource_type {
                query.push(("type".to_string(), resource_type.to_string()));
            }
            if let Some(name) = params.name_filter {
                query.push(("nameFilter".to_string(), name.to_string()));
            }
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

        let Some(filter) = params.filter else {
            return self.client.get(BASE_PATH, &query_refs).await;
        };
        let body = serde_json::to_value(ListResourcesBody {
            category: params.category,
            resource_type: params.resource_type,
            name_filter: params.name_filter,
            filter,
        })?;
        self.client
            .post_with_query(BASE_PATH, &query_refs, &body)
            .await
    }

    /// Get the daily health status history for one resource, oldest first.
    pub async fn health_history(&self, resource_id: &str) -> Result<GetHealthHistoryResponse> {
        let path = format!(
            "{BASE_PATH}/{}/health-history",
            encode_resource_id(resource_id)
        );
        self.client.get(&path, &[]).await
    }

    /// Get the raw resource document for one resource.
    pub async fn raw_data(&self, resource_id: &str) -> Result<GetRawDataResponse> {
        let path = format!("{BASE_PATH}/{}/raw-data", encode_resource_id(resource_id));
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

        // The payload deliberately still carries `resourceType`: the API sends it
        // and the CLI must parse the rest without it.
        let resp: GetAvailableResourceTypesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.resource_types.len(), 2);
        let first = &resp.resource_types[0];
        let category_type = first.category_type.as_ref().unwrap();
        assert_eq!(category_type.category.as_deref(), Some("Hosts"));
        assert_eq!(category_type.type_name.as_deref(), Some("EC2_Instances"));
        assert_eq!(first.label.as_deref(), Some("EC2 Instances"));
    }

    /// `resourceType` is not surfaced in any output format, so it is left out of
    /// [`ResourceTypeMapping`] entirely. The API keeps sending it, so deserializing
    /// must ignore it rather than fail - this guards the absence of
    /// `deny_unknown_fields`, which would turn the extra key into a hard error.
    #[test]
    fn deserialize_types_response_ignores_resource_type() {
        let json = json!({
            "resourceTypes": [
                {
                    "categoryType": { "category": "Hosts", "type": "EC2_Instances" },
                    "resourceType": "aws_ec2_instance",
                    "label": "EC2 Instances",
                    "someFutureField": 42
                }
            ]
        });

        let resp: GetAvailableResourceTypesResponse =
            serde_json::from_value(json).expect("unknown keys must be ignored, not rejected");
        let first = &resp.resource_types[0];
        assert_eq!(first.label.as_deref(), Some("EC2 Instances"));
        assert_eq!(
            first.category_type.as_ref().unwrap().type_name.as_deref(),
            Some("EC2_Instances")
        );
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
        assert_eq!(resp.total_count, 42);
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
        let json = json!({ "totalCount": 0 });
        let resp: GetResourcesResponse = serde_json::from_value(json).unwrap();
        assert!(resp.resources.is_empty());
        assert_eq!(resp.total_count, 0);
    }

    /// `totalCount` is the caller's only stop condition when paging, so a
    /// response missing it must fail loudly rather than default to `0` - which
    /// would report an empty fleet while returning rows.
    #[test]
    fn deserialize_resources_response_requires_total_count() {
        let json = json!({
            "resources": [{ "resourceId": "1001234:host_id=i-abc123", "name": "web-server-1" }]
        });
        let err = serde_json::from_value::<GetResourcesResponse>(json).unwrap_err();
        assert!(
            err.to_string().contains("totalCount"),
            "error should name the missing field, got: {err}"
        );
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

    #[test]
    fn deserialize_health_history_response() {
        let json = json!({
            "healthHistory": [
                { "timestamp": "2026-07-01T00:00:00Z", "status": "Healthy" },
                { "timestamp": "2026-07-02T00:00:00Z", "status": "Critical" },
                { "timestamp": "2026-07-03T00:00:00Z", "status": "Unmonitored" }
            ]
        });

        let resp: GetHealthHistoryResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.health_history.len(), 3);
        assert_eq!(
            resp.health_history[0].timestamp.as_deref(),
            Some("2026-07-01T00:00:00Z")
        );
        assert_eq!(resp.health_history[1].status.as_deref(), Some("Critical"));
    }

    #[test]
    fn deserialize_empty_health_history_response() {
        let json = json!({});
        let resp: GetHealthHistoryResponse = serde_json::from_value(json).unwrap();
        assert!(resp.health_history.is_empty());
    }

    #[test]
    fn deserialize_raw_data_response() {
        let json = json!({
            "rawData": { "host_id": "i-abc123", "tags": { "env": "prod" } }
        });
        let resp: GetRawDataResponse = serde_json::from_value(json).unwrap();
        let doc = resp.raw_data.unwrap();
        assert_eq!(doc["host_id"], "i-abc123");
        assert_eq!(doc["tags"]["env"], "prod");
    }

    #[test]
    fn deserialize_null_raw_data_response() {
        let json = json!({ "rawData": null });
        let resp: GetRawDataResponse = serde_json::from_value(json).unwrap();
        assert!(resp.raw_data.is_none());
    }

    /// a `HashMap` would emit its keys in randomized iteration order - identical
    /// invocations producing different output. `BTreeMap` pins it to sorted order.
    #[test]
    fn resource_columns_serialize_in_stable_sorted_order() {
        let json = json!({
            "resources": [{
                "resourceId": "1001234:host_id=i-abc123",
                "name": "web-server-1",
                "columns": {
                    "region": "us-east-1",
                    "instance_type": "m5.large",
                    "availability_zone": "us-east-1a",
                    "state": "running"
                }
            }],
            "totalCount": 1
        });

        let resp: GetResourcesResponse = serde_json::from_value(json).unwrap();
        let columns = &resp.resources[0].columns;

        assert_eq!(
            columns.keys().collect::<Vec<_>>(),
            vec!["availability_zone", "instance_type", "region", "state"]
        );
        assert_eq!(
            serde_json::to_string(columns).unwrap(),
            r#"{"availability_zone":"us-east-1a","instance_type":"m5.large","region":"us-east-1","state":"running"}"#
        );
    }

    #[test]
    fn encode_resource_id_escapes_reserved_characters() {
        assert_eq!(
            encode_resource_id("1001234:host_id=i-abc|123"),
            "1001234%3Ahost_id%3Di-abc%7C123"
        );
        assert_eq!(encode_resource_id("plain-id_1.2~3"), "plain-id_1.2~3");
    }
}
