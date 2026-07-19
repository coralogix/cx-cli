//! List system and user-defined DataPrime datasets.
//!
//! Calls the archive dataset v2 gRPC services over **gRPC-Web** (HTTP/1.1),
//! matching the same backend RPCs used by Olly's `list_datasets` tool:
//! - `SystemDatasetService/GetSystemDatasets`
//! - `UserDefinedDatasetService/GetUserDefinedDatasets`
//!
//! There is no OpenAPI/REST facade for these RPCs; the web app and Olly use
//! gRPC / gRPC-Web against `api.<region>.coralogix.*`.

use prost::Message;
use serde::Serialize;
use serde_json::Value;

use crate::api_client::CxClient;
use crate::error::{CxError, Result};

const SYSTEM_DATASETS_PATH: &str =
    "/com.coralogix.archive.dataset.v2.SystemDatasetService/GetSystemDatasets";
const USER_DEFINED_DATASETS_PATH: &str =
    "/com.coralogix.archive.dataset.v2.UserDefinedDatasetService/GetUserDefinedDatasets";

/// Match Olly's `list_datasets` truncation limits.
pub const MAX_SYSTEM_DATASET_RESULTS: usize = 50;
pub const MAX_USER_DEFINED_DATASET_RESULTS: usize = 50;

// ── Protobuf messages (field numbers match cx-api-archive dataset v2) ─────────

#[derive(Clone, PartialEq, Message)]
struct GetSystemDatasetsResponse {
    #[prost(message, repeated, tag = "1")]
    datasets: Vec<SystemDataset>,
}

#[derive(Clone, PartialEq, Message)]
struct SystemDataset {
    #[prost(int32, tag = "1")]
    company_id: i32,
    #[prost(string, tag = "3")]
    dataset: String,
    #[prost(bool, tag = "4")]
    ingestion_enabled: bool,
    #[prost(message, optional, tag = "5")]
    created_at: Option<prost_types::Timestamp>,
    #[prost(message, optional, tag = "6")]
    updated_at: Option<prost_types::Timestamp>,
    #[prost(bool, tag = "7")]
    query_enabled: bool,
    #[prost(string, tag = "8")]
    description: String,
    #[prost(string, tag = "9")]
    docs_url: String,
}

#[derive(Clone, PartialEq, Message)]
struct GetUserDefinedDatasetsResponse {
    #[prost(message, repeated, tag = "1")]
    datasets: Vec<UserDefinedDataset>,
}

#[derive(Clone, PartialEq, Message)]
struct UserDefinedDataset {
    #[prost(int32, tag = "1")]
    company_id: i32,
    #[prost(message, optional, tag = "2")]
    dataset: Option<DatasetId>,
    #[prost(message, optional, tag = "3")]
    created_at: Option<prost_types::Timestamp>,
    #[prost(message, optional, tag = "4")]
    updated_at: Option<prost_types::Timestamp>,
    #[prost(string, tag = "5")]
    policy: String,
    #[prost(bool, tag = "6")]
    write_enabled: bool,
}

#[derive(Clone, PartialEq, Message)]
struct DatasetId {
    #[prost(message, optional, tag = "1")]
    dataspace: Option<Dataspace>,
    #[prost(string, tag = "2")]
    dataset: String,
}

#[derive(Clone, PartialEq, Message)]
struct Dataspace {
    #[prost(string, tag = "1")]
    dataspace: String,
}

// ── Public listing types (Olly `list_datasets` shape) ─────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetListing {
    #[serde(rename = "type")]
    pub dataset_type: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetCounts {
    pub system: CountPair,
    pub user: CountPair,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CountPair {
    pub returned: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDatasetsResponse {
    pub always_available_sources: Vec<String>,
    pub dataset_counts: DatasetCounts,
    pub truncated: bool,
    pub datasets: Vec<DatasetListing>,
}

impl ListDatasetsResponse {
    pub fn to_json_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

// ── API ───────────────────────────────────────────────────────────────────────

pub struct DatasetsApi<'a> {
    client: &'a CxClient,
}

impl<'a> DatasetsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// List system + user-defined datasets in the Olly `list_datasets` shape.
    pub async fn list(&self) -> Result<ListDatasetsResponse> {
        let (system, user) =
            tokio::try_join!(self.get_system_datasets(), self.get_user_datasets())?;
        Ok(format_dataset_listings(system, user))
    }

    async fn get_system_datasets(&self) -> Result<Vec<SystemDataset>> {
        let bytes = grpc_web_unary(self.client, SYSTEM_DATASETS_PATH).await?;
        let resp =
            GetSystemDatasetsResponse::decode(bytes.as_slice()).map_err(|e| CxError::Api {
                status: 502,
                message: format!("Failed to decode system datasets response: {e}"),
            })?;
        Ok(resp.datasets)
    }

    async fn get_user_datasets(&self) -> Result<Vec<UserDefinedDataset>> {
        let bytes = grpc_web_unary(self.client, USER_DEFINED_DATASETS_PATH).await?;
        let resp =
            GetUserDefinedDatasetsResponse::decode(bytes.as_slice()).map_err(|e| CxError::Api {
                status: 502,
                message: format!("Failed to decode user-defined datasets response: {e}"),
            })?;
        Ok(resp.datasets)
    }
}

/// Format listings to match Olly's `format_dataset_listings`.
fn format_dataset_listings(
    system_datasets: Vec<SystemDataset>,
    user_defined_datasets: Vec<UserDefinedDataset>,
) -> ListDatasetsResponse {
    let system_total = system_datasets.len();
    let user_total = user_defined_datasets.len();
    let limited_system: Vec<_> = system_datasets
        .into_iter()
        .take(MAX_SYSTEM_DATASET_RESULTS)
        .collect();
    let limited_user: Vec<_> = user_defined_datasets
        .into_iter()
        .take(MAX_USER_DEFINED_DATASET_RESULTS)
        .collect();

    let truncated =
        system_total > MAX_SYSTEM_DATASET_RESULTS || user_total > MAX_USER_DEFINED_DATASET_RESULTS;

    let mut datasets = Vec::with_capacity(limited_system.len() + limited_user.len());
    for ds in &limited_system {
        datasets.push(serialize_system_dataset(ds));
    }
    for ds in &limited_user {
        datasets.push(serialize_user_dataset(ds));
    }

    ListDatasetsResponse {
        always_available_sources: vec!["source logs".into(), "source spans".into()],
        dataset_counts: DatasetCounts {
            system: CountPair {
                returned: limited_system.len(),
                total: system_total,
            },
            user: CountPair {
                returned: limited_user.len(),
                total: user_total,
            },
        },
        truncated,
        datasets,
    }
}

fn serialize_system_dataset(ds: &SystemDataset) -> DatasetListing {
    DatasetListing {
        dataset_type: "system".into(),
        source: format!("system/{}", ds.dataset),
        description: Some(ds.description.clone()),
        query_enabled: Some(ds.query_enabled),
        write_enabled: None,
        created_at: timestamp_to_iso(ds.created_at.as_ref()),
        updated_at: timestamp_to_iso(ds.updated_at.as_ref()),
    }
}

fn serialize_user_dataset(ds: &UserDefinedDataset) -> DatasetListing {
    let (dataspace, name) = match &ds.dataset {
        Some(id) => {
            let space = id
                .dataspace
                .as_ref()
                .map(|d| d.dataspace.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("default");
            (space.to_string(), id.dataset.clone())
        }
        None => ("default".into(), String::new()),
    };
    DatasetListing {
        dataset_type: "user".into(),
        source: format!("{dataspace}/{name}"),
        description: None,
        query_enabled: None,
        write_enabled: Some(ds.write_enabled),
        created_at: timestamp_to_iso(ds.created_at.as_ref()),
        updated_at: timestamp_to_iso(ds.updated_at.as_ref()),
    }
}

fn timestamp_to_iso(ts: Option<&prost_types::Timestamp>) -> Option<String> {
    let ts = ts?;
    let datetime = chrono::DateTime::from_timestamp(ts.seconds, ts.nanos.max(0) as u32)?;
    Some(datetime.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
}

/// Empty unary gRPC-Web request → protobuf response message bytes.
async fn grpc_web_unary(client: &CxClient, path: &str) -> Result<Vec<u8>> {
    // Empty protobuf message framed as gRPC-Web: flags(0) + length(0) + body.
    let request_frame = vec![0, 0, 0, 0, 0];
    let response = client
        .post_bytes(
            path,
            request_frame,
            &[
                ("Content-Type", "application/grpc-web+proto"),
                ("Accept", "application/grpc-web+proto"),
                ("X-Grpc-Web", "1"),
                ("X-User-Agent", "cx-cli"),
            ],
        )
        .await?;
    parse_grpc_web_unary_response(&response)
}

fn parse_grpc_web_unary_response(body: &[u8]) -> Result<Vec<u8>> {
    let mut offset = 0;
    let mut message = Vec::new();
    let mut grpc_status: Option<u32> = None;
    let mut grpc_message: Option<String> = None;

    while offset + 5 <= body.len() {
        let flags = body[offset];
        let len = u32::from_be_bytes([
            body[offset + 1],
            body[offset + 2],
            body[offset + 3],
            body[offset + 4],
        ]) as usize;
        offset += 5;
        if offset + len > body.len() {
            return Err(CxError::Api {
                status: 502,
                message: "Truncated gRPC-Web response frame".into(),
            });
        }
        let frame = &body[offset..offset + len];
        offset += len;

        if flags & 0x80 != 0 {
            // Trailer frame: HTTP/1.1 header block (key: value\r\n...)
            for line in std::str::from_utf8(frame).unwrap_or("").lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some((k, v)) = line.split_once(':') {
                    let key = k.trim().to_ascii_lowercase();
                    let val = v.trim();
                    if key == "grpc-status" {
                        grpc_status = val.parse().ok();
                    } else if key == "grpc-message" {
                        grpc_message = Some(percent_decode(val));
                    }
                }
            }
        } else {
            message.extend_from_slice(frame);
        }
    }

    let status = grpc_status.unwrap_or(0);
    if status != 0 {
        return Err(CxError::Api {
            status: 502,
            message: format!(
                "gRPC error (status {status}): {}",
                grpc_message.unwrap_or_else(|| "unknown error".into())
            ),
        });
    }

    Ok(message)
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(char::from_u32(h * 16 + l).unwrap_or('?'));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_system(name: &str, description: &str) -> SystemDataset {
        SystemDataset {
            company_id: 1,
            dataset: name.into(),
            ingestion_enabled: true,
            created_at: Some(prost_types::Timestamp {
                seconds: 1_700_000_000,
                nanos: 0,
            }),
            updated_at: Some(prost_types::Timestamp {
                seconds: 1_700_000_100,
                nanos: 0,
            }),
            query_enabled: true,
            description: description.into(),
            docs_url: String::new(),
        }
    }

    fn sample_user(dataspace: &str, name: &str, write_enabled: bool) -> UserDefinedDataset {
        UserDefinedDataset {
            company_id: 1,
            dataset: Some(DatasetId {
                dataspace: Some(Dataspace {
                    dataspace: dataspace.into(),
                }),
                dataset: name.into(),
            }),
            created_at: None,
            updated_at: None,
            policy: String::new(),
            write_enabled,
        }
    }

    #[test]
    fn format_matches_olly_shape() {
        let resp = format_dataset_listings(
            vec![sample_system("labs.cases.state_updates", "Case events")],
            vec![sample_user("default", "my_dataset", true)],
        );
        assert_eq!(
            resp.always_available_sources,
            vec!["source logs", "source spans"]
        );
        assert!(!resp.truncated);
        assert_eq!(resp.dataset_counts.system.returned, 1);
        assert_eq!(resp.dataset_counts.user.returned, 1);
        assert_eq!(resp.datasets.len(), 2);

        let system = &resp.datasets[0];
        assert_eq!(system.dataset_type, "system");
        assert_eq!(system.source, "system/labs.cases.state_updates");
        assert_eq!(system.description.as_deref(), Some("Case events"));
        assert_eq!(system.query_enabled, Some(true));
        assert!(system.write_enabled.is_none());

        let user = &resp.datasets[1];
        assert_eq!(user.dataset_type, "user");
        assert_eq!(user.source, "default/my_dataset");
        assert_eq!(user.write_enabled, Some(true));
        assert!(user.description.is_none());
    }

    #[test]
    fn format_truncates_at_limits() {
        let system: Vec<_> = (0..60)
            .map(|i| sample_system(&format!("ds{i}"), "d"))
            .collect();
        let user: Vec<_> = (0..55)
            .map(|i| sample_user("default", &format!("u{i}"), false))
            .collect();
        let resp = format_dataset_listings(system, user);
        assert!(resp.truncated);
        assert_eq!(resp.dataset_counts.system.total, 60);
        assert_eq!(resp.dataset_counts.system.returned, 50);
        assert_eq!(resp.dataset_counts.user.total, 55);
        assert_eq!(resp.dataset_counts.user.returned, 50);
        assert_eq!(resp.datasets.len(), 100);
    }

    #[test]
    fn decode_system_response_roundtrip() {
        let original = GetSystemDatasetsResponse {
            datasets: vec![sample_system("aaa.audit_events", "Audit")],
        };
        let encoded = original.encode_to_vec();
        let decoded = GetSystemDatasetsResponse::decode(encoded.as_slice()).unwrap();
        assert_eq!(decoded.datasets.len(), 1);
        assert_eq!(decoded.datasets[0].dataset, "aaa.audit_events");
        assert!(decoded.datasets[0].query_enabled);
    }

    #[test]
    fn parse_grpc_web_data_and_trailers() {
        let msg = GetSystemDatasetsResponse {
            datasets: vec![sample_system("engine.queries", "Queries")],
        }
        .encode_to_vec();
        let mut body = Vec::new();
        body.push(0); // data frame
        body.extend_from_slice(&(msg.len() as u32).to_be_bytes());
        body.extend_from_slice(&msg);
        let trailers = b"grpc-status: 0\r\ngrpc-message: \r\n";
        body.push(0x80);
        body.extend_from_slice(&(trailers.len() as u32).to_be_bytes());
        body.extend_from_slice(trailers);

        let decoded_bytes = parse_grpc_web_unary_response(&body).unwrap();
        let decoded = GetSystemDatasetsResponse::decode(decoded_bytes.as_slice()).unwrap();
        assert_eq!(decoded.datasets[0].dataset, "engine.queries");
    }

    #[test]
    fn parse_grpc_web_error_status() {
        let trailers = b"grpc-status: 7\r\ngrpc-message: Permission%20denied\r\n";
        let mut body = Vec::new();
        body.push(0x80);
        body.extend_from_slice(&(trailers.len() as u32).to_be_bytes());
        body.extend_from_slice(trailers);
        let err = parse_grpc_web_unary_response(&body).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("status 7"), "{msg}");
        assert!(msg.contains("Permission denied"), "{msg}");
    }

    #[test]
    fn list_response_json_keys() {
        let resp = format_dataset_listings(
            vec![sample_system("x", "d")],
            vec![sample_user("default", "y", false)],
        );
        let v = resp.to_json_value();
        assert!(v.get("alwaysAvailableSources").is_some());
        assert!(v.get("datasetCounts").is_some());
        assert!(v.get("truncated").is_some());
        assert!(v.get("datasets").is_some());
        assert!(v.get("truncated").is_some());
    }
}
