//! Schema Store keypaths HTTP client.
//!
//! - [`LIST_KEYPATHS_PATH`] and [`GET_KEYPATH_SAMPLES_PATH`] (`POST`), JSON in the proto3 JSON
//!   form of `com.coralogixapis.schemastore.v1.KeypathsService` (cx-api-schema-store).
//! - Requests over a server-side cap (time range, `limit`, dataset count) are rejected with 400;
//!   they are never clamped.

use serde::{Deserialize, Serialize};

use crate::api_client::CxClient;
use crate::error::Result as CxResult;

pub const LIST_KEYPATHS_PATH: &str = "/api/v1/schema-store/keypaths/list";
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum KeypathMatchMode {
    #[serde(rename = "KEYPATH_MATCH_MODE_CONTAINS")]
    Contains,
    #[serde(rename = "KEYPATH_MATCH_MODE_STARTS_WITH")]
    StartsWith,
    #[serde(rename = "KEYPATH_MATCH_MODE_EXACT")]
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum KeypathScope {
    #[serde(rename = "KEYPATH_SCOPE_USER_DATA")]
    UserData,
    #[serde(rename = "KEYPATH_SCOPE_LABEL")]
    Label,
    #[serde(rename = "KEYPATH_SCOPE_METADATA")]
    Metadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeypathFilter {
    pub value: String,
    pub match_mode: KeypathMatchMode,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DimensionMatch {
    pub keypath: String,
    /// Any of these matches.
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListKeypathsRequest {
    pub datasets: Vec<NamedDataset>,
    pub time_range: TimeRange,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keypath_filter: Option<KeypathFilter>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<KeypathScope>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dimension_filter: Vec<DimensionMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetKeypathSamplesRequest {
    pub datasets: Vec<NamedDataset>,
    pub keypath: String,
    pub time_range: TimeRange,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dimension_filter: Vec<DimensionMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One listed keypath: its DataPrime spelling and the DataPrime types it held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keypath {
    pub keypath: String,
    #[serde(default)]
    pub types: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListKeypathsResponse {
    #[serde(default)]
    pub keypaths: Vec<Keypath>,
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

    pub async fn list(&self, request: &ListKeypathsRequest) -> CxResult<ListKeypathsResponse> {
        self.client
            .post(LIST_KEYPATHS_PATH, &serde_json::to_value(request)?)
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
    fn serializes_full_list_request() {
        let request = ListKeypathsRequest {
            datasets: logs(),
            time_range: range(),
            keypath_filter: Some(KeypathFilter {
                value: "$d.kubernetes".into(),
                match_mode: KeypathMatchMode::StartsWith,
            }),
            scopes: vec![KeypathScope::UserData, KeypathScope::Label],
            dimension_filter: vec![DimensionMatch {
                keypath: "$l.applicationName".into(),
                values: vec!["checkout".into(), "payments".into()],
            }],
            limit: Some(50),
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "datasets": [{"dataspace": "default", "dataset": "logs"}],
                "timeRange": {"start": "2026-11-06T10:00:00Z", "end": "2026-11-06T11:00:00Z"},
                "keypathFilter": {
                    "value": "$d.kubernetes",
                    "matchMode": "KEYPATH_MATCH_MODE_STARTS_WITH"
                },
                "scopes": ["KEYPATH_SCOPE_USER_DATA", "KEYPATH_SCOPE_LABEL"],
                "dimensionFilter": [
                    {"keypath": "$l.applicationName", "values": ["checkout", "payments"]}
                ],
                "limit": 50
            })
        );
    }

    #[test]
    fn omits_unset_list_fields() {
        let request = ListKeypathsRequest {
            datasets: logs(),
            time_range: range(),
            keypath_filter: None,
            scopes: vec![],
            dimension_filter: vec![],
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
            dimension_filter: vec![],
            limit: Some(5),
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "datasets": [{"dataspace": "default", "dataset": "logs"}],
                "keypath": "$m.severity",
                "timeRange": {"start": "2026-11-06T10:00:00Z", "end": "2026-11-06T11:00:00Z"},
                "limit": 5
            })
        );
    }

    #[test]
    fn deserializes_list_response() {
        let resp: ListKeypathsResponse = serde_json::from_value(json!({
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
        let list: ListKeypathsResponse = serde_json::from_value(json!({})).unwrap();
        assert!(list.keypaths.is_empty());
        assert!(!list.truncated);
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
