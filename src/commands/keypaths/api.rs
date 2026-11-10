//! Schema Store keypath search HTTP client.
//!
//! - [`SEARCH_KEYPATHS_PATH`] and [`GET_KEYPATH_SAMPLES_PATH`] (`POST`), JSON in the proto3 JSON
//!   form of `com.coralogixapis.schemastore.v1.SchemaService` (cx-api-schema-store).
//! - Requests over a server-side cap (time range, `limit`, dataset count, filter size) are rejected
//!   with 400; they are never clamped.

use serde::{Deserialize, Serialize};

use crate::api_client::CxClient;
use crate::error::Result as CxResult;

pub const SEARCH_KEYPATHS_PATH: &str = "/api/v1/schema-store/keypaths/search";
pub const GET_KEYPATH_SAMPLES_PATH: &str = "/api/v1/schema-store/keypaths/samples";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NamedDataset {
    pub dataspace: String,
    pub dataset: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeRange {
    pub start: String,
    pub end: String,
}

/// One condition on a keypath; serializes as `{"prefix": "..."}`, `{"all": {"filters": [...]}}`, ...
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeypathFilter {
    Exact(String),
    Prefix(String),
    Contains(String),
    All(KeypathFilters),
    Any(KeypathFilters),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeypathFilters {
    pub filters: Vec<KeypathFilter>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DimensionFilter {
    Match(DimensionMatch),
    /// Every one holds.
    All(DimensionFilters),
    /// At least one holds; the server accepts matches on one dimension only.
    Any(DimensionFilters),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DimensionFilters {
    pub filters: Vec<DimensionFilter>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DimensionMatch {
    pub keypath: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchKeypathsRequest {
    pub datasets: Vec<NamedDataset>,
    pub time_range: TimeRange,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keypath_filter: Option<KeypathFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension_filter: Option<DimensionFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetKeypathSamplesRequest {
    pub datasets: Vec<NamedDataset>,
    pub keypath: String,
    pub time_range: TimeRange,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension_filter: Option<DimensionFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One found keypath: its DataPrime spelling and the DataPrime types it held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeypathSearchResult {
    pub keypath: String,
    #[serde(default)]
    pub types: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchKeypathsResponse {
    #[serde(default)]
    pub keypaths: Vec<KeypathSearchResult>,
    /// proto3 JSON omits `false`.
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
pub struct GetKeypathSamplesResponse {
    #[serde(default)]
    pub samples: Vec<String>,
}

pub struct KeypathsApi<'a> {
    client: &'a CxClient,
}

impl<'a> KeypathsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn search(
        &self,
        request: &SearchKeypathsRequest,
    ) -> CxResult<SearchKeypathsResponse> {
        self.client
            .post(SEARCH_KEYPATHS_PATH, &serde_json::to_value(request)?)
            .await
    }

    pub async fn samples(
        &self,
        request: &GetKeypathSamplesRequest,
    ) -> CxResult<GetKeypathSamplesResponse> {
        self.client
            .post(GET_KEYPATH_SAMPLES_PATH, &serde_json::to_value(request)?)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn logs() -> Vec<NamedDataset> {
        vec![NamedDataset {
            dataspace: "default".into(),
            dataset: "logs".into(),
        }]
    }

    fn range() -> TimeRange {
        TimeRange {
            start: "2026-11-06T10:00:00Z".into(),
            end: "2026-11-06T11:00:00Z".into(),
        }
    }

    #[test]
    fn serializes_full_search_request() {
        let request = SearchKeypathsRequest {
            datasets: logs(),
            time_range: range(),
            keypath_filter: Some(KeypathFilter::All(KeypathFilters {
                filters: vec![
                    KeypathFilter::Any(KeypathFilters {
                        filters: vec![
                            KeypathFilter::Prefix("$d.kubernetes".into()),
                            KeypathFilter::Prefix("$d.k8s".into()),
                        ],
                    }),
                    KeypathFilter::Contains("pod".into()),
                ],
            })),
            dimension_filter: Some(DimensionFilter::Any(DimensionFilters {
                filters: vec![
                    DimensionFilter::Match(DimensionMatch {
                        keypath: "$l.applicationName".into(),
                        value: "checkout".into(),
                    }),
                    DimensionFilter::Match(DimensionMatch {
                        keypath: "$l.applicationName".into(),
                        value: "payments".into(),
                    }),
                ],
            })),
            limit: Some(50),
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "datasets": [{"dataspace": "default", "dataset": "logs"}],
                "timeRange": {"start": "2026-11-06T10:00:00Z", "end": "2026-11-06T11:00:00Z"},
                "keypathFilter": {"all": {"filters": [
                    {"any": {"filters": [{"prefix": "$d.kubernetes"}, {"prefix": "$d.k8s"}]}},
                    {"contains": "pod"}
                ]}},
                "dimensionFilter": {"any": {"filters": [
                    {"match": {"keypath": "$l.applicationName", "value": "checkout"}},
                    {"match": {"keypath": "$l.applicationName", "value": "payments"}}
                ]}},
                "limit": 50
            })
        );
    }

    #[test]
    fn omits_unset_search_fields() {
        let request = SearchKeypathsRequest {
            datasets: logs(),
            time_range: range(),
            keypath_filter: None,
            dimension_filter: None,
            limit: None,
        };
        let value = serde_json::to_value(&request).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["datasets", "timeRange"]);
    }

    #[test]
    fn serializes_samples_request() {
        let request = GetKeypathSamplesRequest {
            datasets: logs(),
            keypath: "$m.severity".into(),
            time_range: range(),
            dimension_filter: Some(DimensionFilter::All(DimensionFilters {
                filters: vec![DimensionFilter::Match(DimensionMatch {
                    keypath: "$l.subsystemName".into(),
                    value: "api".into(),
                })],
            })),
            limit: Some(5),
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "datasets": [{"dataspace": "default", "dataset": "logs"}],
                "keypath": "$m.severity",
                "timeRange": {"start": "2026-11-06T10:00:00Z", "end": "2026-11-06T11:00:00Z"},
                "dimensionFilter": {"all": {"filters": [
                    {"match": {"keypath": "$l.subsystemName", "value": "api"}}
                ]}},
                "limit": 5
            })
        );
    }

    #[test]
    fn deserializes_search_response() {
        let resp: SearchKeypathsResponse = serde_json::from_value(json!({
            "keypaths": [
                {"keypath": "$d.kubernetes.pod_name", "types": ["string"]},
                {"keypath": "$d.tags['http.method']", "types": ["number", "string"]}
            ],
            "truncated": true
        }))
        .unwrap();
        assert_eq!(resp.keypaths.len(), 2);
        assert_eq!(resp.keypaths[1].types, ["number", "string"]);
        assert!(resp.truncated);
    }

    #[test]
    fn deserializes_empty_responses() {
        let search: SearchKeypathsResponse = serde_json::from_value(json!({})).unwrap();
        assert!(search.keypaths.is_empty());
        assert!(!search.truncated);
        let samples: GetKeypathSamplesResponse = serde_json::from_value(json!({})).unwrap();
        assert!(samples.samples.is_empty());
    }

    #[test]
    fn deserializes_samples_response() {
        let resp: GetKeypathSamplesResponse =
            serde_json::from_value(json!({"samples": ["ERROR", "INFO"]})).unwrap();
        assert_eq!(resp.samples, ["ERROR", "INFO"]);
    }
}
