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
            let dataprime_path = serialize_path_for_query(&r.path_array)?;
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

fn serialize_path_for_query(path_array: &[String]) -> Option<String> {
    let prefix = path_array.first()?;
    if !matches!(prefix.as_str(), "$d" | "$m" | "$l") {
        return None;
    }

    let mut path = prefix.clone();
    for key in &path_array[1..] {
        if is_complex_key(key) {
            path.push_str("['");
            path.push_str(&stringify_path_key(key));
            path.push_str("']");
        } else {
            path.push('.');
            path.push_str(key);
        }
    }

    Some(path)
}

fn is_complex_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return true;
    };

    !(first.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

fn stringify_path_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn serializes_simple_path_with_dot_notation() {
        assert_eq!(
            serialize_path_for_query(&path(&["$d", "http", "status_code"])).as_deref(),
            Some("$d.http.status_code")
        );
    }

    #[test]
    fn serializes_complex_keys_with_bracket_notation() {
        assert_eq!(
            serialize_path_for_query(&path(&["$d", "resource.type", "service name", "_id"]))
                .as_deref(),
            Some("$d['resource.type']['service name']['_id']")
        );
    }

    #[test]
    fn treats_letter_prefixed_keys_as_simple() {
        assert_eq!(
            serialize_path_for_query(&path(&["$l", "service_1"])).as_deref(),
            Some("$l.service_1")
        );
    }

    #[test]
    fn escapes_complex_key_literals() {
        assert_eq!(
            serialize_path_for_query(&path(&["$d", "service's", r"path\key"])).as_deref(),
            Some(r"$d['service\'s']['path\\key']")
        );
    }

    #[test]
    fn rejects_invalid_top_level_prefix() {
        assert_eq!(serialize_path_for_query(&path(&["$x", "field"])), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_search_item_with_extra_fields() {
        // Test that we can deserialize API responses with dataset_scope and labels
        let json = r#"{
            "path_array": ["http", "status_code"],
            "description": "HTTP status code",
            "similarity_score": 0.95,
            "dataset_scope": "USER_DATA",
            "labels": {"category": "http", "type": "numeric"}
        }"#;

        let item: FieldSearchItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.path_array, vec!["http", "status_code"]);
        assert_eq!(item.description, "HTTP status code");
        assert_eq!(item.similarity_score, 0.95);
        assert_eq!(item.dataset_scope, Some("USER_DATA".to_string()));
        assert!(item.labels.is_object());
    }

    #[test]
    fn test_field_search_item_without_extra_fields() {
        // Test backward compatibility - API responses without dataset_scope/labels
        let json = r#"{
            "path_array": ["http", "status_code"],
            "description": "HTTP status code",
            "similarity_score": 0.95
        }"#;

        let item: FieldSearchItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.path_array, vec!["http", "status_code"]);
        assert_eq!(item.description, "HTTP status code");
        assert_eq!(item.similarity_score, 0.95);
        assert_eq!(item.dataset_scope, None);
        assert!(item.labels.is_null());
    }

    #[test]
    fn test_field_search_item_with_null_dataset_scope() {
        // Test that explicit null is handled correctly
        let json = r#"{
            "path_array": ["http", "status_code"],
            "description": "HTTP status code",
            "similarity_score": 0.95,
            "dataset_scope": null,
            "labels": null
        }"#;

        let item: FieldSearchItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.dataset_scope, None);
        assert!(item.labels.is_null());
    }

    #[test]
    fn test_field_search_item_with_metadata_scope() {
        // Test METADATA scope value
        let json = r#"{
            "path_array": ["applicationName"],
            "description": "Application name",
            "similarity_score": 0.88,
            "dataset_scope": "METADATA",
            "labels": {}
        }"#;

        let item: FieldSearchItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.dataset_scope, Some("METADATA".to_string()));
    }

    #[test]
    fn test_serialize_path_for_query() {
        assert_eq!(
            serialize_path_for_query(&["http".to_string(), "status_code".to_string()]),
            "http.status_code"
        );
        assert_eq!(
            serialize_path_for_query(&["d".to_string()]),
            "d"
        );
        assert_eq!(serialize_path_for_query(&[]), "");
    }
}
