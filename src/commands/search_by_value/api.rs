//! Olly Knowledge Base search-by-value HTTP client.
//!
//! - Path: [`SEARCH_BY_VALUE_PATH`] (`POST`), body: `query`, `dataset_type`, `limit`, `offset`.
//! - Platform ingress (`olly-knowledge-base` values-reader): gateway permission
//!   `legacy-archive-queries:Execute` (permission id 40). Dataset checks remain in the service.

use serde::{Deserialize, Serialize};

use crate::api_client::CxClient;
use crate::error::Result as CxResult;

const SEARCH_BY_VALUE_PATH: &str = "/api/v1/search-by-value";

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
/// * `dataset` must be `"logs"`, `"spans"`, or `"all"` (sent as `dataset_type` in the JSON body).
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
    client.post(SEARCH_BY_VALUE_PATH, &body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_single_result() {
        let json = r#"{
            "matches": [
                {
                    "key_matched": "$d.status",
                    "value": "payment_failed",
                    "similarity_score": 0.87
                }
            ],
            "total_hits": 1
        }"#;
        let resp: SearchByValueResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.total_hits, 1);
        assert_eq!(resp.matches.len(), 1);
        let r = &resp.matches[0];
        assert_eq!(r.key_matched, "$d.status");
        assert_eq!(r.value, "payment_failed");
        assert_eq!(r.similarity_score, 0.87);
    }

    #[test]
    fn deserializes_empty_matches() {
        let json = r#"{"matches": [], "total_hits": 0}"#;
        let resp: SearchByValueResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.total_hits, 0);
        assert!(resp.matches.is_empty());
    }

    #[test]
    fn deserializes_multiple_results() {
        let json = r#"{
            "matches": [
                {"key_matched": "$d.env", "value": "production", "similarity_score": 0.95},
                {"key_matched": "$d.region", "value": "eu-west-1", "similarity_score": 0.72}
            ],
            "total_hits": 2
        }"#;
        let resp: SearchByValueResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.matches.len(), 2);
        assert_eq!(resp.matches[0].key_matched, "$d.env");
        assert_eq!(resp.matches[1].similarity_score, 0.72);
    }
}
