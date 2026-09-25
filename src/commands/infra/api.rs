use std::collections::BTreeMap;

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api_client::CxClient;
use crate::error::Result;

pub(super) const BASE_PATH: &str = "/mgmt/api/infrastructure/resources/v1";

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
    #[serde(default)]
    pub values: Vec<String>,
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
    #[serde(default)]
    pub health_policies: Vec<HealthPolicyData>,
}

#[derive(Debug, Deserialize)]
pub struct HealthPolicyData {
    pub id: Option<String>,
    pub status: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceHealthHistory {
    pub resource_id: Option<String>,
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
    pub version_timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummarizeConfigChangesResponse {
    #[serde(default)]
    pub results: Vec<ResourceChangeData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceChangeData {
    pub resource_id: Option<String>,
    pub source: Option<String>,
    pub outcome: Option<String>,
    pub last_change: Option<String>,
    pub change_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffConfigChangesResponse {
    #[serde(default)]
    pub results: Vec<ResourceDiffData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDiffData {
    pub resource_id: Option<String>,
    pub source: Option<String>,
    pub outcome: Option<String>,
    pub compared_from: Option<String>,
    pub compared_to: Option<String>,
    #[serde(default)]
    pub changes: Vec<FieldChangeData>,
}

#[derive(Debug, Deserialize)]
pub struct FieldChangeData {
    pub field: Option<String>,
    /// Raw JSON, so numbers and booleans survive rather than becoming strings.
    #[serde(default)]
    pub before: Value,
    #[serde(default)]
    pub after: Value,
}

pub struct ListResourcesParams<'p> {
    pub category: Option<&'p str>,
    pub resource_type: Option<&'p str>,
    pub filter: Option<&'p Filter>,
    pub start_row: Option<i64>,
    pub end_row: Option<i64>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Filter {
    Match(FieldMatch),
    Bool(BoolFilter),
}

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
struct ResourceIdsBody<'p> {
    resource_ids: &'p [&'p str],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigChangesBody<'p> {
    from: &'p str,
    to: &'p str,
    resource_ids: &'p [&'p str],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListResourcesBody<'p> {
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<&'p str>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    resource_type: Option<&'p str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<&'p Filter>,
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

pub struct ConfigChangesParams<'p> {
    pub from: &'p str,
    pub to: &'p str,
    pub resource_ids: &'p [&'p str],
}

impl ConfigChangesParams<'_> {
    fn to_body(&self) -> Result<Value> {
        Ok(serde_json::to_value(ConfigChangesBody {
            from: self.from,
            to: self.to,
            resource_ids: self.resource_ids,
        })?)
    }
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

    /// List resources matching a filter, with a `startRow`/`endRow` page window.
    pub async fn list(&self, params: &ListResourcesParams<'_>) -> Result<GetResourcesResponse> {
        let mut query: Vec<(String, String)> = Vec::new();
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

        let body = serde_json::to_value(ListResourcesBody {
            category: params.category,
            resource_type: params.resource_type,
            filter: params.filter,
        })?;
        self.client
            .post_with_headers(BASE_PATH, Some(&query_refs), &body, &[])
            .await
    }

    /// Get the daily health status history for several resources.
    /// Health history for each resource is sorted oldest first.
    /// Ids the API cannot parse are left out and duplicates are read once.
    pub async fn health_history(
        &self,
        resource_ids: &[&str],
    ) -> Result<Vec<ResourceHealthHistory>> {
        let path = format!("{BASE_PATH}/health-history");
        let body = serde_json::to_value(ResourceIdsBody { resource_ids })?;
        self.client.post(&path, &body).await
    }

    /// Which of these resources changed over the window, most recent first.
    /// A resource that did not change is absent from the answer.
    pub async fn config_changes(
        &self,
        params: &ConfigChangesParams<'_>,
    ) -> Result<SummarizeConfigChangesResponse> {
        let path = format!("{BASE_PATH}/configuration/summary");
        self.client.post(&path, &params.to_body()?).await
    }

    /// What changed in each resource's configuration over the window, field by
    /// field.
    pub async fn config_diff(
        &self,
        params: &ConfigChangesParams<'_>,
    ) -> Result<DiffConfigChangesResponse> {
        let path = format!("{BASE_PATH}/configuration/diff");
        self.client.post(&path, &params.to_body()?).await
    }

    /// Get the raw resource document for one resource.
    ///
    /// `timestamp` asks for the newest version at or before that instant.
    /// The response names the version it actually is.
    pub async fn raw_data(
        &self,
        resource_id: &str,
        timestamp: Option<&str>,
    ) -> Result<GetRawDataResponse> {
        let path = format!("{BASE_PATH}/{}/raw-data", encode_resource_id(resource_id));
        let query: Vec<(&str, &str)> = timestamp.map(|t| ("timestamp", t)).into_iter().collect();
        self.client.get(&path, &query).await
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
    fn deserialize_resource_health_policies() {
        let json = json!({
            "resources": [{
                "resourceId": "1001234:host_id=i-abc123",
                "name": "web-server-1",
                "healthPolicies": [
                    { "id": "p-1", "status": "critical", "name": "Deployment has unavailable replicas" },
                    { "id": "p-2", "status": "healthy", "name": "Pod CPU utilization high" },
                    { "id": "p-3", "status": "pending", "name": "" }
                ]
            }],
            "totalCount": 1
        });

        let resp: GetResourcesResponse = serde_json::from_value(json).unwrap();
        let policies = &resp.resources[0].health_policies;
        assert_eq!(policies.len(), 3);
        assert_eq!(policies[0].id.as_deref(), Some("p-1"));
        assert_eq!(policies[0].status.as_deref(), Some("critical"));
        assert_eq!(
            policies[0].name.as_deref(),
            Some("Deployment has unavailable replicas")
        );
        assert_eq!(policies[2].status.as_deref(), Some("pending"));
        // The catalog did not resolve this one; the API sends "" rather than omitting it.
        assert_eq!(policies[2].name.as_deref(), Some(""));
    }

    /// The three statuses are what the API sends today. A fourth must reach the
    /// user as itself rather than failing the row it arrived on - which is what
    /// an enum with three variants would do.
    #[test]
    fn deserialize_resource_health_policy_with_an_unknown_status() {
        let json = json!({
            "resources": [{
                "resourceId": "1001234:host_id=i-abc123",
                "healthPolicies": [{ "id": "p-1", "status": "degraded", "name": "New Policy" }]
            }],
            "totalCount": 1
        });

        let resp: GetResourcesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(
            resp.resources[0].health_policies[0].status.as_deref(),
            Some("degraded")
        );
    }

    /// A resource no policy applies to, and a response from before the field
    /// existed, must both read as "no policies" rather than as a parse failure.
    #[test]
    fn deserialize_resource_without_health_policies() {
        let empty = json!({
            "resources": [{ "resourceId": "id-1", "healthPolicies": [] }],
            "totalCount": 1
        });
        let resp: GetResourcesResponse = serde_json::from_value(empty).unwrap();
        assert!(resp.resources[0].health_policies.is_empty());

        let absent = json!({
            "resources": [{ "resourceId": "id-1" }],
            "totalCount": 1
        });
        let resp: GetResourcesResponse = serde_json::from_value(absent).unwrap();
        assert!(resp.resources[0].health_policies.is_empty());
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
        let json = json!([
            {
                "resourceId": "1001234:host_id=i-abc123",
                "healthHistory": [
                    { "timestamp": "2026-07-01T00:00:00Z", "status": "Healthy" },
                    { "timestamp": "2026-07-02T00:00:00Z", "status": "Critical" },
                    { "timestamp": "2026-07-03T00:00:00Z", "status": "Unmonitored" }
                ]
            },
            {
                "resourceId": "1001234:host_id=i-def456",
                "healthHistory": [
                    { "timestamp": "2026-07-01T00:00:00Z", "status": "Healthy" }
                ]
            }
        ]);

        let resp: Vec<ResourceHealthHistory> = serde_json::from_value(json).unwrap();
        assert_eq!(resp.len(), 2);
        assert_eq!(
            resp[0].resource_id.as_deref(),
            Some("1001234:host_id=i-abc123")
        );
        assert_eq!(resp[0].health_history.len(), 3);
        assert_eq!(
            resp[0].health_history[0].timestamp.as_deref(),
            Some("2026-07-01T00:00:00Z")
        );
        assert_eq!(
            resp[0].health_history[1].status.as_deref(),
            Some("Critical")
        );
        assert_eq!(resp[1].health_history.len(), 1);
    }

    /// A resource with no samples still gets an entry, and the whole answer can
    /// be empty. Neither is an error.
    #[test]
    fn deserialize_empty_health_history_response() {
        let one_without_samples = json!([{ "resourceId": "id-1" }]);
        let resp: Vec<ResourceHealthHistory> = serde_json::from_value(one_without_samples).unwrap();
        assert!(resp[0].health_history.is_empty());

        let nothing = json!([]);
        let resp: Vec<ResourceHealthHistory> = serde_json::from_value(nothing).unwrap();
        assert!(resp.is_empty());
    }

    /// The API declares `deny_unknown_fields` on this body, so the key has to be
    /// exactly `resourceIds` or every request is a 400.
    #[test]
    fn serialize_resource_ids_body() {
        let ids = ["1001234:host_id=i-abc123", "1001234:host_id=i-def456"];
        let body = serde_json::to_value(ResourceIdsBody {
            resource_ids: &ids[..],
        })
        .unwrap();

        assert_eq!(
            body,
            json!({
                "resourceIds": ["1001234:host_id=i-abc123", "1001234:host_id=i-def456"]
            })
        );
    }

    #[test]
    fn deserialize_raw_data_response() {
        let json = json!({
            "rawData": { "host_id": "i-abc123", "tags": { "env": "prod" } },
            "versionTimestamp": "2026-09-03T13:26:58Z"
        });
        let resp: GetRawDataResponse = serde_json::from_value(json).unwrap();
        let doc = resp.raw_data.unwrap();
        assert_eq!(doc["host_id"], "i-abc123");
        assert_eq!(doc["tags"]["env"], "prod");
        assert_eq!(
            resp.version_timestamp.as_deref(),
            Some("2026-09-03T13:26:58Z")
        );
    }

    #[test]
    fn deserialize_null_raw_data_response() {
        let json = json!({ "rawData": null, "versionTimestamp": null });
        let resp: GetRawDataResponse = serde_json::from_value(json).unwrap();
        assert!(resp.raw_data.is_none());
        assert!(resp.version_timestamp.is_none());
    }

    /// A document with no version attached is a different thing from no
    /// document, so the two fields must stay independent.
    #[test]
    fn deserialize_raw_data_without_a_version() {
        let json = json!({ "rawData": { "host_id": "i-abc123" } });
        let resp: GetRawDataResponse = serde_json::from_value(json).unwrap();
        assert!(resp.raw_data.is_some());
        assert!(resp.version_timestamp.is_none());
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
    fn deserialize_summarize_config_changes_response() {
        let json = json!({
            "results": [
                { "resourceId": "7000098:a=frontend", "source": "OTEL",
                  "outcome": "changed", "lastChange": "2026-09-06T12:43:20Z", "changeCount": 7 },
                { "resourceId": "7000098:a=cart", "source": "OTEL",
                  "outcome": "created", "lastChange": "2026-09-06T11:10:00Z", "changeCount": 1 }
            ]
        });

        let resp: SummarizeConfigChangesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.results[0].outcome.as_deref(), Some("changed"));
        assert_eq!(resp.results[0].change_count, Some(7));
        assert_eq!(resp.results[1].outcome.as_deref(), Some("created"));
    }

    /// A resource that did not change is absent, so an empty list is the normal
    /// "nothing changed" answer rather than a failure.
    #[test]
    fn deserialize_empty_summarize_response() {
        let resp: SummarizeConfigChangesResponse = serde_json::from_value(json!({})).unwrap();
        assert!(resp.results.is_empty());
    }

    #[test]
    fn deserialize_diff_config_changes_response() {
        let json = json!({
            "results": [{
                "resourceId": "7000098:a=frontend",
                "source": "OTEL",
                "outcome": "changed",
                "comparedFrom": "2026-09-05T09:20:00Z",
                "comparedTo": "2026-09-06T11:59:01Z",
                "changes": [
                    { "field": "spec.replicas", "before": 3, "after": 10 },
                    { "field": "spec.containers[app].image",
                      "before": "shop:1.4.0", "after": "shop:1.5.0" },
                    { "field": "spec.paused", "before": false, "after": true }
                ]
            }]
        });

        let resp: DiffConfigChangesResponse = serde_json::from_value(json).unwrap();
        let diff = &resp.results[0];
        assert_eq!(diff.compared_from.as_deref(), Some("2026-09-05T09:20:00Z"));
        assert_eq!(diff.changes.len(), 3);

        // Raw JSON, so a number stays a number and a bool stays a bool.
        assert_eq!(diff.changes[0].before, json!(3));
        assert_eq!(diff.changes[0].after, json!(10));
        assert_eq!(diff.changes[1].after, json!("shop:1.5.0"));
        assert_eq!(diff.changes[2].before, json!(false));
    }

    /// `comparedFrom` / `comparedTo` are null where there is no such version,
    /// and an outcome can carry no changes at all.
    #[test]
    fn deserialize_diff_without_a_comparison() {
        let json = json!({
            "results": [{
                "resourceId": "7000098:a=cart",
                "source": "OTEL",
                "outcome": "created",
                "comparedFrom": null,
                "comparedTo": "2026-09-06T11:59:01Z",
                "changes": []
            }]
        });

        let resp: DiffConfigChangesResponse = serde_json::from_value(json).unwrap();
        let diff = &resp.results[0];
        assert!(diff.compared_from.is_none());
        assert!(diff.changes.is_empty());
        assert_eq!(diff.outcome.as_deref(), Some("created"));
    }

    /// A field set to JSON null is a real value, not a missing one, so an added
    /// or removed field must survive rather than deserialize into nothing.
    #[test]
    fn deserialize_field_change_with_a_null_side() {
        let json = json!({
            "results": [{
                "resourceId": "7000098:a=frontend",
                "outcome": "changed",
                "changes": [{ "field": "spec.sidecar", "before": null, "after": { "image": "x" } }]
            }]
        });

        let resp: DiffConfigChangesResponse = serde_json::from_value(json).unwrap();
        let change = &resp.results[0].changes[0];
        assert_eq!(change.before, Value::Null);
        assert_eq!(change.after, json!({ "image": "x" }));
    }

    /// The API declares `deny_unknown_fields`, so all three keys have to be
    /// spelled exactly this way or every request is a 400.
    #[test]
    fn serialize_config_changes_body() {
        let ids = ["7000098:a=frontend", "7000098:a=cart"];
        let params = ConfigChangesParams {
            from: "2026-09-06T11:00:00.000000000Z",
            to: "2026-09-06T13:00:00.000000000Z",
            resource_ids: &ids[..],
        };

        assert_eq!(
            params.to_body().unwrap(),
            json!({
                "from": "2026-09-06T11:00:00.000000000Z",
                "to": "2026-09-06T13:00:00.000000000Z",
                "resourceIds": ["7000098:a=frontend", "7000098:a=cart"]
            })
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
