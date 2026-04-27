//! Coralogix Fields and Metrics Semantic Search HTTP API.

use serde::{Deserialize, Deserializer, Serialize};

use crate::api_client::CxClient;
use crate::error::Result as CxResult;

/// One result row returned by semantic field lookup (`semantic-search/fields`).
#[derive(Debug, Serialize)]
pub struct SemanticFieldResult {
    /// Full DataPrime path, e.g. `$d.http.status_code`
    pub dataprime_path: String,
    /// DataPrime namespace prefix: `$d`, `$m`, or `$l`
    pub top_level_key: String,
    /// Remaining path segments after the namespace prefix
    pub path: Vec<String>,
    pub description: String,
    /// Semantic similarity (higher is more similar, range 0–1)
    pub similarity: f64,
}

/// Deserialize `null` or a JSON array into `Vec` (API may send `"metric_suffixes": null`).
fn deserialize_nullable_string_list<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<Vec<String>>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// One result row from `semantic-search/metrics`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SemanticMetricResult {
    pub metric_name: String,
    pub description: String,
    pub metric_type: String,
    #[serde(default, deserialize_with = "deserialize_nullable_string_list")]
    pub metric_suffixes: Vec<String>,
    /// Semantic similarity (higher is more similar, range 0–1)
    pub similarity_score: f64,
}

#[derive(Debug, Deserialize)]
struct FieldsHttpResponse {
    results: Vec<FieldSearchItem>,
}

#[derive(Debug, Deserialize)]
struct FieldSearchItem {
    path_array: Vec<String>,
    description: String,
    similarity_score: f64,
    /// Present in API JSON; retained for forward compatibility.
    #[serde(default)]
    #[allow(dead_code)]
    dataset_scope: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    labels: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct SemanticMetricsHttpResponse {
    results: Vec<SemanticMetricResult>,
}

/// Natural-language search over log/span fields (`POST /api/v1/semantic-search/fields`).
///
/// * `dataset` must be `"logs"` or `"spans"` (`dataset_type` in the API).
pub async fn semantic_field_lookup(
    client: &CxClient,
    text: &str,
    dataset: &str,
    limit: u32,
) -> CxResult<Vec<SemanticFieldResult>> {
    let limit = limit.clamp(1, 100);
    let body = serde_json::json!({
        "query": text,
        "dataset_type": dataset,
        "limit": limit,
    });
    let parsed: FieldsHttpResponse = client.post("/api/v1/semantic-search/fields", &body).await?;

    let results = parsed
        .results
        .into_iter()
        .filter_map(|r| {
            let first = r.path_array.first()?;
            if !matches!(first.as_str(), "$d" | "$m" | "$l") {
                eprintln!(
                    "Warning: unexpected path_array prefix '{}', skipping",
                    first
                );
                return None;
            }
            let top_level_key = first.clone();
            let path = r.path_array[1..].to_vec();
            let dataprime_path = serialize_path_for_query(&r.path_array);
            Some(SemanticFieldResult {
                dataprime_path,
                top_level_key,
                path,
                description: r.description,
                similarity: r.similarity_score.clamp(0.0, 1.0),
            })
        })
        .collect();

    Ok(results)
}

/// Natural-language search over metrics (`POST /api/v1/semantic-search/metrics`).
pub async fn semantic_metric_lookup(
    client: &CxClient,
    text: &str,
    limit: u32,
) -> CxResult<Vec<SemanticMetricResult>> {
    let limit = limit.clamp(1, 100);
    let body = serde_json::json!({
        "query": text,
        "limit": limit,
    });
    let parsed: SemanticMetricsHttpResponse = client
        .post("/api/v1/semantic-search/metrics", &body)
        .await?;

    Ok(parsed.results)
}

fn serialize_path_for_query(path_array: &[String]) -> String {
    path_array.join(".")
}
