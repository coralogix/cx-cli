use std::collections::BTreeMap;

use anyhow::{bail, Result as AnyResult};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api_client::CxClient;
use crate::error::Result;

const BASE_PATH: &str = "/v2/entities";

/// Full proto enum names accepted by the service-catalog v2 HTTP API, in the
/// same order as `com.coralogix.catalog.v2.EntityType`. `ENTITY_TYPE_UNSPECIFIED`
/// is deliberately excluded - it is a protobuf zero-value, never a meaningful
/// entity type to query.
const VALID_ENTITY_TYPES: &[&str] = &[
    "ENTITY_TYPE_TRANSACTION",
    "ENTITY_TYPE_SERVICE",
    "ENTITY_TYPE_DATABASE",
    "ENTITY_TYPE_OPERATION",
    "ENTITY_TYPE_DATABASE_OPERATION",
    "ENTITY_TYPE_JVM",
    "ENTITY_TYPE_JVM_GC",
    "ENTITY_TYPE_K8S_POD",
];

/// Normalizes a user-supplied entity type into the full proto enum name the v2
/// HTTP API's path segment accepts - e.g. `service`, `K8S_POD`, or
/// `ENTITY_TYPE_K8S_POD` all become `ENTITY_TYPE_K8S_POD`. The backend rejects
/// anything else (including the short forms), so this must run before the id
/// ever reaches a URL.
pub fn normalize_entity_type(input: &str) -> AnyResult<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("entity type must not be empty");
    }
    let upper = trimmed.to_uppercase().replace('-', "_");
    let name = if upper.starts_with("ENTITY_TYPE_") {
        upper
    } else {
        format!("ENTITY_TYPE_{upper}")
    };
    if !VALID_ENTITY_TYPES.contains(&name.as_str()) {
        bail!(
            "unknown entity type '{input}'; supported: service, database, operation, \
             database-operation, jvm, jvm-gc, k8s-pod, transaction"
        );
    }
    Ok(name)
}

/// Entity ids travel as a URL path segment and are otherwise free-form
/// (service/pod names), so percent-encode everything except RFC 3986
/// unreserved characters - mirrors `infra::api::encode_resource_id`.
const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

pub fn encode_entity_id(entity_id: &str) -> String {
    utf8_percent_encode(entity_id, PATH_SEGMENT_ENCODE_SET).to_string()
}

// ── Response types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityTypeInfo {
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    pub id: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListEntityTypesResponse {
    #[serde(default)]
    pub entity_types: Vec<EntityTypeInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMetadata {
    pub id: Option<String>,
    pub display_name: Option<String>,
    pub unit: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EntityTypeMetadata {
    pub entity_id: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub columns: Vec<ColumnMetadata>,
    #[serde(default)]
    pub default_columns: Vec<String>,
    #[serde(default)]
    pub groupable_labels: Vec<String>,
    pub group_by_limit: Option<i32>,
    #[serde(default)]
    pub default_group_by: Vec<String>,
    #[serde(default)]
    pub filterable_labels: Vec<String>,
    #[serde(default)]
    pub required_filters: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentActivity {
    pub name: Option<String>,
    pub last_seen: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityItem {
    pub name: Option<String>,
    pub system: Option<String>,
    pub last_seen: Option<String>,
    #[serde(default)]
    pub deployments: Vec<String>,
    #[serde(default)]
    pub environments: Vec<EnvironmentActivity>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListEntitiesResponse {
    #[serde(default)]
    pub entities: Vec<EntityItem>,
}

/// Raw `EntitiesDataResult` - a protobuf `oneof` of `table` or `timeseries`.
/// `values`/`series` are kept as raw JSON: `ColumnValue` is itself a `oneof`
/// with a dozen possible shapes, so flattening happens in `mod.rs` rather than
/// through a fully-typed struct here (mirrors the shared Python implementation
/// in `cx-olly`'s `libs/common/src/common/tools/service_catalog_tools.py`).
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EntitiesDataResult {
    pub table: Option<TableData>,
    pub timeseries: Option<TimeseriesData>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TableData {
    #[serde(default)]
    pub rows: Vec<TableRow>,
    #[serde(default)]
    pub columns: Vec<ColumnMetadata>,
}

#[derive(Debug, Deserialize, Default)]
pub struct TableRow {
    #[serde(default)]
    pub identity: BTreeMap<String, String>,
    /// Column id -> raw `ColumnResult` JSON (`{"value": ColumnValue}` or
    /// `{"error": ColumnError}`).
    #[serde(default)]
    pub values: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimeseriesData {
    #[serde(default)]
    pub series: Vec<Value>,
    #[serde(default)]
    pub columns: Vec<ColumnMetadata>,
    #[serde(default)]
    pub total_series_count: i32,
}

// ── Request body ────────────────────────────────────────────────────────────────

/// One `ApmFilter`: a label name plus the values it must match. `metric_label_name`
/// (for labels that live under a different name in the metric series) is not
/// exposed by the CLI - it is a niche field with no natural flag, and every
/// other product surface treats it as optional.
#[derive(Debug)]
pub struct ApmFilterInput {
    pub label_name: String,
    pub label_values: Vec<String>,
}

/// Parameters for [`ServiceCatalogApi::entities_data`] / [`ServiceCatalogApi::entity_data`].
///
/// `limit` / `sort_column` / `sort_order` only apply to `entities_data` (TABLE
/// aggregation) - callers building an entity-data request simply leave them `None`.
pub struct DataParams<'p> {
    pub start: i64,
    pub end: i64,
    pub columns: &'p [String],
    pub group_by: &'p [String],
    pub filters: &'p [ApmFilterInput],
    /// Full proto enum name, e.g. `DATA_AGGREGATION_TYPE_TABLE`.
    pub data_aggregation_type: &'p str,
    pub limit: Option<i32>,
    pub sort_column: Option<&'p str>,
    /// Full proto enum name, e.g. `SORT_ORDER_ASCENDING`.
    pub sort_order: Option<&'p str>,
}

/// Builds the JSON body shared by `GetEntitiesDataRequest` / `GetEntityDataRequest`.
///
/// `entityType` (and `entityId` for the single-entity endpoint) are deliberately
/// omitted: the HTTP route overrides both from the URL path regardless of what
/// the body carries, so sending them would be dead weight.
fn build_data_body(params: &DataParams<'_>) -> Value {
    let mut body = json!({
        "timeRange": { "start": params.start, "end": params.end },
        "dataAggregationType": params.data_aggregation_type,
        "columns": params.columns.iter().map(|c| json!({ "columnId": c })).collect::<Vec<_>>(),
        "groupBy": params.group_by,
        "filters": params.filters.iter().map(|f| json!({
            "labelName": f.label_name,
            "labelValues": f.label_values,
        })).collect::<Vec<_>>(),
    });
    let obj = body.as_object_mut().expect("body is always an object");
    if let Some(limit) = params.limit {
        obj.insert("limit".to_string(), json!(limit));
    }
    if let Some(sort_column) = params.sort_column {
        obj.insert("sortColumn".to_string(), json!(sort_column));
    }
    if let Some(sort_order) = params.sort_order {
        obj.insert("sortOrder".to_string(), json!(sort_order));
    }
    body
}

// ── API client ───────────────────────────────────────────────────────────────────

pub struct ServiceCatalogApi<'a> {
    client: &'a CxClient,
}

impl<'a> ServiceCatalogApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// `ListEntityTypes` - the entity types this account has service-catalog data for.
    pub async fn list_entity_types(&self) -> Result<ListEntityTypesResponse> {
        self.client.get(BASE_PATH, &[]).await
    }

    /// `GetEntityTypeMetadata` - the columns/labels schema for one entity type.
    /// `entity_type` must already be normalized (see [`normalize_entity_type`]).
    pub async fn entity_type_schema(&self, entity_type: &str) -> Result<EntityTypeMetadata> {
        let path = format!("{BASE_PATH}/{entity_type}/metadata");
        self.client.get(&path, &[]).await
    }

    /// `ListEntities` - the known entities (e.g. service names) of one entity type.
    /// `entity_type` must already be normalized.
    pub async fn list_entities(&self, entity_type: &str) -> Result<ListEntitiesResponse> {
        let path = format!("{BASE_PATH}/{entity_type}");
        self.client.get(&path, &[]).await
    }

    /// `GetEntitiesData` - column data across every entity of one type.
    /// `entity_type` must already be normalized.
    pub async fn entities_data(
        &self,
        entity_type: &str,
        params: &DataParams<'_>,
    ) -> Result<EntitiesDataResult> {
        let path = format!("{BASE_PATH}/{entity_type}/data");
        self.client.post(&path, &build_data_body(params)).await
    }

    /// `GetEntityData` - column data for exactly one named entity (drilldown).
    /// `entity_type` must already be normalized; `entity_id` is percent-encoded here.
    pub async fn entity_data(
        &self,
        entity_type: &str,
        entity_id: &str,
        params: &DataParams<'_>,
    ) -> Result<EntitiesDataResult> {
        let path = format!(
            "{BASE_PATH}/{entity_type}/data/entity/{}",
            encode_entity_id(entity_id)
        );
        self.client.post(&path, &build_data_body(params)).await
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── normalize_entity_type ────────────────────────────────────────────────

    #[test]
    fn normalize_entity_type_accepts_short_lowercase_form() {
        assert_eq!(
            normalize_entity_type("service").unwrap(),
            "ENTITY_TYPE_SERVICE"
        );
    }

    #[test]
    fn normalize_entity_type_accepts_hyphenated_short_form() {
        assert_eq!(
            normalize_entity_type("k8s-pod").unwrap(),
            "ENTITY_TYPE_K8S_POD"
        );
        assert_eq!(
            normalize_entity_type("database-operation").unwrap(),
            "ENTITY_TYPE_DATABASE_OPERATION"
        );
    }

    #[test]
    fn normalize_entity_type_accepts_full_proto_name_any_case() {
        assert_eq!(
            normalize_entity_type("ENTITY_TYPE_JVM").unwrap(),
            "ENTITY_TYPE_JVM"
        );
        assert_eq!(
            normalize_entity_type("entity_type_jvm").unwrap(),
            "ENTITY_TYPE_JVM"
        );
    }

    #[test]
    fn normalize_entity_type_trims_whitespace() {
        assert_eq!(
            normalize_entity_type("  service  ").unwrap(),
            "ENTITY_TYPE_SERVICE"
        );
    }

    #[test]
    fn normalize_entity_type_rejects_empty() {
        let err = normalize_entity_type("  ").unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn normalize_entity_type_rejects_unknown_type() {
        let err = normalize_entity_type("pod").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown entity type 'pod'"), "got: {msg}");
        assert!(msg.contains("k8s-pod"), "got: {msg}");
    }

    /// `UNSPECIFIED` is a protobuf zero-value, not a real entity type - never
    /// something a query should be able to ask for.
    #[test]
    fn normalize_entity_type_rejects_unspecified() {
        assert!(normalize_entity_type("unspecified").is_err());
        assert!(normalize_entity_type("ENTITY_TYPE_UNSPECIFIED").is_err());
    }

    #[test]
    fn encode_entity_id_escapes_reserved_characters() {
        assert_eq!(encode_entity_id("checkout/api"), "checkout%2Fapi");
        assert_eq!(encode_entity_id("plain-name_1.2~3"), "plain-name_1.2~3");
    }

    // ── Response deserialization ─────────────────────────────────────────────

    #[test]
    fn deserialize_list_entity_types_response() {
        let json = json!({
            "entityTypes": [
                {
                    "type": "ENTITY_TYPE_SERVICE",
                    "id": "service",
                    "displayName": "Service",
                    "description": "APM services"
                }
            ]
        });
        let resp: ListEntityTypesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.entity_types.len(), 1);
        assert_eq!(
            resp.entity_types[0].entity_type.as_deref(),
            Some("ENTITY_TYPE_SERVICE")
        );
        assert_eq!(resp.entity_types[0].id.as_deref(), Some("service"));
    }

    #[test]
    fn deserialize_empty_list_entity_types_response() {
        let resp: ListEntityTypesResponse = serde_json::from_value(json!({})).unwrap();
        assert!(resp.entity_types.is_empty());
    }

    #[test]
    fn deserialize_entity_type_metadata() {
        let json = json!({
            "entityId": "service",
            "displayName": "Service",
            "description": "APM services",
            "columns": [{ "id": "latency_p99", "displayName": "P99 Latency", "unit": "UNIT_MILLISECONDS" }],
            "defaultColumns": ["latency_p99"],
            "groupableLabels": ["environment"],
            "groupByLimit": 3,
            "defaultGroupBy": [],
            "filterableLabels": ["environment"],
            "requiredFilters": []
        });
        let resp: EntityTypeMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(resp.entity_id.as_deref(), Some("service"));
        assert_eq!(resp.columns.len(), 1);
        assert_eq!(resp.columns[0].id.as_deref(), Some("latency_p99"));
        assert_eq!(resp.group_by_limit, Some(3));
    }

    #[test]
    fn deserialize_empty_entity_type_metadata() {
        let resp: EntityTypeMetadata = serde_json::from_value(json!({})).unwrap();
        assert!(resp.columns.is_empty());
        assert!(resp.group_by_limit.is_none());
    }

    #[test]
    fn deserialize_list_entities_response() {
        let json = json!({
            "entities": [
                {
                    "name": "checkout",
                    "system": "kubernetes",
                    "lastSeen": "2026-07-01T00:00:00Z",
                    "deployments": ["checkout-v1"],
                    "environments": [{ "name": "prod", "lastSeen": "2026-07-01T00:00:00Z" }]
                }
            ]
        });
        let resp: ListEntitiesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.entities.len(), 1);
        assert_eq!(resp.entities[0].name.as_deref(), Some("checkout"));
        assert_eq!(resp.entities[0].environments.len(), 1);
        assert_eq!(
            resp.entities[0].environments[0].name.as_deref(),
            Some("prod")
        );
    }

    #[test]
    fn deserialize_empty_list_entities_response() {
        let resp: ListEntitiesResponse = serde_json::from_value(json!({})).unwrap();
        assert!(resp.entities.is_empty());
    }

    #[test]
    fn deserialize_entities_data_result_table() {
        let json = json!({
            "table": {
                "rows": [
                    {
                        "identity": { "name": "checkout" },
                        "values": { "latency_p99": { "value": { "metric": 42.5 } } }
                    }
                ],
                "columns": [{ "id": "latency_p99" }]
            }
        });
        let resp: EntitiesDataResult = serde_json::from_value(json).unwrap();
        let table = resp.table.expect("table should be present");
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].identity.get("name").unwrap(), "checkout");
        assert!(resp.timeseries.is_none());
    }

    #[test]
    fn deserialize_entities_data_result_timeseries() {
        let json = json!({
            "timeseries": {
                "series": [{ "columnId": "latency_p99", "datapoints": [] }],
                "columns": [{ "id": "latency_p99" }],
                "totalSeriesCount": 1
            }
        });
        let resp: EntitiesDataResult = serde_json::from_value(json).unwrap();
        let timeseries = resp.timeseries.expect("timeseries should be present");
        assert_eq!(timeseries.series.len(), 1);
        assert_eq!(timeseries.total_series_count, 1);
        assert!(resp.table.is_none());
    }

    #[test]
    fn deserialize_empty_entities_data_result() {
        let resp: EntitiesDataResult = serde_json::from_value(json!({})).unwrap();
        assert!(resp.table.is_none());
        assert!(resp.timeseries.is_none());
    }

    // ── build_data_body ───────────────────────────────────────────────────────

    #[test]
    fn build_data_body_includes_required_fields_only_by_default() {
        let filters = vec![];
        let columns = vec!["latency_p99".to_string()];
        let group_by = vec![];
        let params = DataParams {
            start: 1000,
            end: 2000,
            columns: &columns,
            group_by: &group_by,
            filters: &filters,
            data_aggregation_type: "DATA_AGGREGATION_TYPE_TABLE",
            limit: None,
            sort_column: None,
            sort_order: None,
        };
        let body = build_data_body(&params);
        assert_eq!(body["timeRange"]["start"], 1000);
        assert_eq!(body["timeRange"]["end"], 2000);
        assert_eq!(body["dataAggregationType"], "DATA_AGGREGATION_TYPE_TABLE");
        assert_eq!(body["columns"], json!([{ "columnId": "latency_p99" }]));
        assert!(body.get("limit").is_none());
        assert!(body.get("sortColumn").is_none());
        assert!(body.get("sortOrder").is_none());
        assert!(
            body.get("entityType").is_none(),
            "entityType travels in the URL path, not the body"
        );
    }

    #[test]
    fn build_data_body_includes_table_controls_when_set() {
        let filters = vec![ApmFilterInput {
            label_name: "environment".to_string(),
            label_values: vec!["prod".to_string(), "staging".to_string()],
        }];
        let columns = vec!["latency_p99".to_string()];
        let group_by = vec!["environment".to_string()];
        let params = DataParams {
            start: 1000,
            end: 2000,
            columns: &columns,
            group_by: &group_by,
            filters: &filters,
            data_aggregation_type: "DATA_AGGREGATION_TYPE_TABLE",
            limit: Some(10),
            sort_column: Some("latency_p99"),
            sort_order: Some("SORT_ORDER_DESCENDING"),
        };
        let body = build_data_body(&params);
        assert_eq!(body["limit"], 10);
        assert_eq!(body["sortColumn"], "latency_p99");
        assert_eq!(body["sortOrder"], "SORT_ORDER_DESCENDING");
        assert_eq!(body["groupBy"], json!(["environment"]));
        assert_eq!(
            body["filters"],
            json!([{ "labelName": "environment", "labelValues": ["prod", "staging"] }])
        );
    }
}
