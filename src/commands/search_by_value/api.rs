use serde::{Deserialize, Serialize};

use crate::api_client::CxClient;
use crate::error::Result as CxResult;

/// Frequent Search logs high-read — gateway path (`dataset`: logs, or `all` via logs ingress).
const SEARCH_BY_VALUE_PATH_LOGS: &str = "/api/v1/search-by-value/logs";
/// Frequent Search spans high-read — gateway path (`dataset`: spans).
const SEARCH_BY_VALUE_PATH_SPANS: &str = "/api/v1/search-by-value/spans";

#[inline]
fn path_for_dataset(dataset: &str) -> &'static str {
    if dataset.trim().eq_ignore_ascii_case("spans") {
        SEARCH_BY_VALUE_PATH_SPANS
    } else {
        // logs, all, or unknown → logs ingress (same gateway permission family as `logs`; unknown defaults safely).
        SEARCH_BY_VALUE_PATH_LOGS
    }
}

/// One result row returned by the values semantic-search API.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchByValueResult {
    /// Field key that matched the query value content.
    pub key_matched: String,
    /// A value from that field that is semantically similar to the query.
    pub value: String,
    /// Semantic similarity score (0–1; higher is more similar).
    pub similarity_score: f64,
}

#[derive(Debug, Deserialize)]
pub struct SearchByValueResponse {
    pub matches: Vec<SearchByValueResult>,
    pub total_hits: u64,
}

/// Fuzzy-search log/span field keys by value content.
///
/// * HTTP path: `.../search-by-value/logs` for `logs` and `all`; `.../search-by-value/spans` for `spans` (gateway permissions).
/// * `limit` is clamped to 1–100.
pub async fn search_by_value(
    client: &CxClient,
    query: &str,
    dataset: &str,
    limit: u32,
    offset: u32,
) -> CxResult<SearchByValueResponse> {
    let limit = limit.clamp(1, 100);
    let body = serde_json::json!({
        "query": query,
        "dataset_type": dataset,
        "limit": limit,
        "offset": offset,
    });
    client.post(path_for_dataset(dataset), &body).await
}
