//! Coralogix Semantic Search HTTP API (`ng-api-http` gateway).
//! See Olly Knowledge Base integration guide: `POST /api/v1/semantic-search/{fields,metrics}`.

use anyhow::{Context, Result};
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

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
fn deserialize_nullable_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
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

/// Map profile API base (`https://api.<region>.coralogix.com`) to the public gateway host
/// used by the Semantic Search REST API (`https://ng-api-http.<region>.coralogix.com`).
pub fn semantic_search_gateway_from_api_endpoint(api_endpoint: &str) -> String {
    let base = api_endpoint.trim_end_matches('/');
    if base.contains("ng-api-http") {
        return base.to_string();
    }
    if let Some(rest) = base.strip_prefix("https://api.") {
        return format!("https://ng-api-http.{rest}");
    }
    if let Some(rest) = base.strip_prefix("http://api.") {
        return format!("http://ng-api-http.{rest}");
    }
    base.to_string()
}

fn build_default_headers(api_key: &str, team_id: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .context("Invalid API key format for Authorization header")?,
    );
    // Do not set Content-Type here — `.json()` on the request sets it (avoids duplicate headers).
    headers.insert(
        header::HeaderName::from_static("cgx-team-id"),
        HeaderValue::from_str(team_id).context("Invalid cgx-team-id header value")?,
    );
    Ok(headers)
}

/// POST JSON to the Semantic Search API (`ng-api-http` gateway) and deserialize the body as `R`.
///
/// `profile_api_endpoint` is the profile’s Coralogix API base (e.g. `https://api.eu2.coralogix.com`);
/// it is mapped to the public gateway host. `path` should start with `/`, e.g.
/// `/api/v1/semantic-search/metrics`.
pub async fn semantic_search_post<R: DeserializeOwned>(
    profile_api_endpoint: &str,
    path: &str,
    api_key: &str,
    team_id: &str,
    body: serde_json::Value,
) -> Result<R> {
    team_id
        .parse::<u32>()
        .with_context(|| format!("cgx-team-id must be a numeric company ID, got: {team_id}"))?;

    let base = semantic_search_gateway_from_api_endpoint(profile_api_endpoint);
    let base = base.trim_end_matches('/');
    let url = format!("{base}{path}");

    let headers = build_default_headers(api_key, team_id)?;
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .user_agent(concat!("cx-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client for semantic search")?;

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;

    let status = resp.status();
    let response_text = resp
        .text()
        .await
        .context("read semantic search response body")?;

    if !status.is_success() {
        anyhow::bail!("semantic search failed: HTTP {status} — {response_text} (URL: {url})");
    }

    serde_json::from_str(&response_text)
        .with_context(|| format!("invalid JSON from semantic search: {response_text}"))
}

/// Natural-language search over log/span fields (`POST /api/v1/semantic-search/fields`).
///
/// * `dataset` must be `"logs"` or `"spans"` (`dataset_type` in the API).
pub async fn semantic_field_lookup(
    endpoint: &str,
    api_key: &str,
    team_id: &str,
    text: &str,
    dataset: &str,
    limit: u32,
) -> Result<Vec<SemanticFieldResult>> {
    let limit = limit.clamp(1, 100);
    let body = serde_json::json!({
        "query": text,
        "dataset_type": dataset,
        "limit": limit,
    });
    let parsed: FieldsHttpResponse = semantic_search_post(
        endpoint,
        "/api/v1/semantic-search/fields",
        api_key,
        team_id,
        body,
    )
    .await?;

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
    endpoint: &str,
    api_key: &str,
    team_id: &str,
    text: &str,
    limit: u32,
) -> Result<Vec<SemanticMetricResult>> {
    let limit = limit.clamp(1, 100);
    let body = serde_json::json!({
        "query": text,
        "limit": limit,
    });
    let parsed: SemanticMetricsHttpResponse = semantic_search_post(
        endpoint,
        "/api/v1/semantic-search/metrics",
        api_key,
        team_id,
        body,
    )
    .await?;

    Ok(parsed.results)
}

fn serialize_path_for_query(path_array: &[String]) -> String {
    path_array.join(".")
}
