//! REST client for the Coralogix datasets listing API (dataengine datasets v1).
//!
//! Backs `cx datasets list`. The endpoint is served by the openapi-facade
//! mount every management command uses, fronting the
//! `com.coralogixapis.dataengine.datasets.v1.DatasetService/ListDatasets`
//! gRPC route. It returns every dataset the calling key's team can query —
//! the fully qualified `<dataspace>/<dataset>` names DataPrime `source`
//! commands need — latest version only, read-enabled datasets only.
//! An optional DPXL filter narrows the list.

use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetMetadata {
    pub team_id: Option<u32>,
    pub dataspace: Option<String>,
    pub dataset: Option<String>,
    pub version: Option<u32>,
    pub pillar: Option<String>,
    pub entity_type: Option<String>,
}

impl DatasetMetadata {
    /// Fully qualified DataPrime source name: `<dataspace>/<dataset>`.
    pub fn source(&self) -> String {
        format!(
            "{}/{}",
            self.dataspace.as_deref().unwrap_or("-"),
            self.dataset.as_deref().unwrap_or("-")
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDatasetsResponse {
    #[serde(default)]
    pub datasets: Vec<DatasetMetadata>,
}

// --- API ---

const DATASETS_BASE: &str = "/mgmt/openapi/5/dataengine/datasets/v1";

pub struct DatasetsApi<'a> {
    client: &'a CxClient,
}

impl<'a> DatasetsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// List queryable datasets for the calling key's team.
    ///
    /// `filter` is an optional DPXL expression including its version tag,
    /// e.g. `"<v1> $d.pillar == 'logs'"`, over the fields `$d.dataspace`,
    /// `$d.dataset`, `$d.pillar`, `$d.entityType` and `$d.teamId`. A filter
    /// the API rejects fails with a 400 surfaced through the standard error
    /// ladder.
    pub async fn list(&self, filter: Option<&str>) -> Result<ListDatasetsResponse> {
        let body: Value = match filter {
            Some(f) => serde_json::json!({ "dpxlFilter": { "value": f } }),
            None => serde_json::json!({}),
        };
        self.client.post(DATASETS_BASE, &body).await
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
            "datasets": [
                {
                    "teamId": 17266,
                    "dataspace": "default",
                    "dataset": "logs",
                    "version": 1,
                    "pillar": "logs",
                    "entityType": "logs"
                },
                {
                    "teamId": 17266,
                    "dataspace": "default",
                    "dataset": "rum.events",
                    "version": 1,
                    "pillar": "logs",
                    "entityType": "browserLogs/v2"
                },
                {
                    "teamId": 17266,
                    "dataspace": "system",
                    "dataset": "labs.limitViolations",
                    "version": 1,
                    "pillar": "logs",
                    "entityType": "labsLimitViolations"
                }
            ]
        });

        let resp: ListDatasetsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.datasets.len(), 3);

        let first = &resp.datasets[0];
        assert_eq!(first.team_id, Some(17266));
        assert_eq!(first.dataspace.as_deref(), Some("default"));
        assert_eq!(first.dataset.as_deref(), Some("logs"));
        assert_eq!(first.version, Some(1));
        assert_eq!(first.pillar.as_deref(), Some("logs"));
        assert_eq!(first.entity_type.as_deref(), Some("logs"));
        assert_eq!(first.source(), "default/logs");

        assert_eq!(resp.datasets[1].source(), "default/rum.events");
        assert_eq!(resp.datasets[2].source(), "system/labs.limitViolations");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListDatasetsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.datasets.is_empty());
    }

    #[test]
    fn deserialize_missing_optional_fields() {
        let json = json!({
            "datasets": [
                { "teamId": 17266, "dataset": "orphan" }
            ]
        });
        let resp: ListDatasetsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.datasets.len(), 1);
        let ds = &resp.datasets[0];
        assert_eq!(ds.dataspace, None);
        assert_eq!(ds.version, None);
        assert_eq!(ds.pillar, None);
        assert_eq!(ds.entity_type, None);
        assert_eq!(ds.source(), "-/orphan");
    }
}
