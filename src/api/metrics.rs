use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Range query types ---

#[derive(Debug, Serialize, Deserialize)]
pub struct MetricRangeSeries {
    pub metric: std::collections::HashMap<String, String>,
    pub values: Vec<[Value; 2]>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PromQueryRangeData {
    #[serde(rename = "resultType")]
    pub result_type: String,
    pub result: Vec<MetricRangeSeries>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PromQueryRangeResponse {
    pub status: String,
    pub data: PromQueryRangeData,
}

// --- Instant query types ---

#[derive(Debug, Serialize, Deserialize)]
pub struct MetricInstantSample {
    pub metric: std::collections::HashMap<String, String>,
    /// [timestamp, value] pair
    pub value: [Value; 2],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PromQueryInstantData {
    #[serde(rename = "resultType")]
    pub result_type: String,
    pub result: Vec<MetricInstantSample>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PromQueryInstantResponse {
    pub status: String,
    pub data: PromQueryInstantData,
}

// --- Label values / labels list types ---

#[derive(Debug, Serialize, Deserialize)]
pub struct PromLabelValuesResponse {
    pub status: String,
    pub data: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PromLabelsResponse {
    pub status: String,
    pub data: Vec<String>,
}

// --- API ---

pub struct MetricsApi<'a> {
    client: &'a CxClient,
}

impl<'a> MetricsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// Execute a PromQL instant query.
    pub async fn query(&self, expr: &str, time: Option<&str>) -> Result<PromQueryInstantResponse> {
        let mut params: Vec<(&str, &str)> = vec![("query", expr)];
        if let Some(t) = time {
            params.push(("time", t));
        }
        self.client.get("/metrics/api/v1/query", &params).await
    }

    /// Execute a PromQL range query.
    pub async fn query_range(
        &self,
        expr: &str,
        start: &str,
        end: &str,
        step: &str,
    ) -> Result<PromQueryRangeResponse> {
        self.client
            .get(
                "/metrics/api/v1/query_range",
                &[
                    ("query", expr),
                    ("start", start),
                    ("end", end),
                    ("step", step),
                ],
            )
            .await
    }

    /// Fetch all metric names from the label __name__ values endpoint.
    pub async fn metric_names(&self) -> Result<PromLabelValuesResponse> {
        self.client
            .get("/metrics/api/v1/label/__name__/values", &[])
            .await
    }

    /// Fetch all label names for a specific metric.
    pub async fn labels_for_metric(&self, metric: &str) -> Result<PromLabelsResponse> {
        self.client
            .get("/metrics/api/v1/labels", &[("match[]", metric)])
            .await
    }
}
