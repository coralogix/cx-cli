use anyhow::{Context, Result};
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::{Deserialize, Deserializer, Serialize};
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, ClientTlsConfig};
use tonic::Request;

use crate::api::openai::generate_embedding;

pub mod proto {
    tonic::include_proto!("com.coralogix.schemastore.v1");
}

use proto::{
    schema_store_olly_lookup_service_client::SchemaStoreOllyLookupServiceClient,
    DatasetV2, Embedding, EmbeddingV1, NamedDataset, SemanticFieldLookupRequest,
    {dataset_v2::Dataset, embedding::Embedding as EmbeddingOneof},
};

const CLIENT_ID: &str = "olly_internal";

/// One result row returned by the semantic field lookup.
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

/// One result row returned by the semantic metric lookup (REST `semantic-search/metrics` payload).
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
struct SemanticMetricsHttpResponse {
    results: Vec<SemanticMetricResult>,
}

/// Perform a semantic field lookup against the SchemaStore gRPC service.
///
/// * `endpoint`       — Base API URL (e.g. `https://api.eu2.coralogix.com`)
/// * `api_key`        — Coralogix API key (Bearer token)
/// * `team_id`        — Coralogix team / company ID (sent as `cgx-team-id`)
/// * `openai_api_key` — OpenAI API key for generating the embedding
/// * `text`           — Free-text query to embed and search
/// * `dataset`        — `"logs"` or `"spans"`
/// * `limit`          — Maximum number of results
pub async fn semantic_field_lookup(
    endpoint: &str,
    api_key: &str,
    team_id: &str,
    openai_api_key: &str,
    text: &str,
    dataset: &str,
    limit: u32,
) -> Result<Vec<SemanticFieldResult>> {
    let company_id: u32 = team_id
        .parse()
        .with_context(|| format!("cgx-team-id must be a numeric company ID, got: {team_id}"))?;

    let embedding_values = generate_embedding(text, openai_api_key)
        .await
        .context("Failed to generate OpenAI embedding")?;

    let mut client = build_client(endpoint, api_key, team_id).await?;

    let request = SemanticFieldLookupRequest {
        company_id,
        embedding: Some(Embedding {
            embedding: Some(EmbeddingOneof::V1(EmbeddingV1 {
                values: embedding_values,
            })),
        }),
        dataset_filter: vec![DatasetV2 {
            dataset: Some(Dataset::NamedDataset(NamedDataset {
                company_id,
                dataspace: "default".to_string(),
                dataset: dataset.to_string(),
                version: 1,
            })),
        }],
        limit,
    };

    let response = client
        .semantic_field_lookup(request)
        .await
        .context("SchemaStore gRPC call failed")?;

    let results = response
        .into_inner()
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
                // The API stores cosine distance; invert to get similarity.
                similarity: (1.0 - r.similarity_score as f64).clamp(0.0, 1.0),
            })
        })
        .collect();

    Ok(results)
}

/// Perform semantic metric lookup via the public Semantic Search REST API
/// (`POST /api/v1/semantic-search/metrics`). See Coralogix Olly KB integration guide.
///
/// * `endpoint` — Profile base URL (e.g. `https://api.eu2.coralogix.com`); mapped to
///   `https://ng-api-http.<region>.coralogix.com` unless the host already contains `ng-api-http`.
/// * `api_key`  — Coralogix API key (`Authorization: Bearer …`)
/// * `team_id`  — Company ID (`cgx-team-id` header)
/// * `text`     — Natural-language query (1–2000 chars)
/// * `limit`    — Max results (clamped to 1–100 per API)
pub async fn semantic_metric_lookup(
    endpoint: &str,
    api_key: &str,
    team_id: &str,
    text: &str,
    limit: u32,
) -> Result<Vec<SemanticMetricResult>> {
    team_id
        .parse::<u32>()
        .with_context(|| format!("cgx-team-id must be a numeric company ID, got: {team_id}"))?;

    let base = ng_api_http_base(endpoint);
    let url = format!("{base}/api/v1/semantic-search/metrics");
    let limit = limit.clamp(1, 100);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .context("Invalid API key format for Authorization header")?,
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::HeaderName::from_static("cgx-team-id"),
        HeaderValue::from_str(team_id).context("Invalid cgx-team-id header value")?,
    );
    headers.insert(
        header::USER_AGENT,
        HeaderValue::from_static(concat!("cx-cli/", env!("CARGO_PKG_VERSION"))),
    );
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .context("failed to build HTTP client for semantic metric search")?;

    let body = serde_json::json!({
        "query": text,
        "limit": limit,
    });

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
        .context("read semantic-search/metrics response body")?;

    if !status.is_success() {
        anyhow::bail!("semantic metric search failed: HTTP {status} — {response_text}");
    }

    let parsed: SemanticMetricsHttpResponse = serde_json::from_str(&response_text)
        .with_context(|| format!("invalid JSON from semantic-search/metrics: {response_text}"))?;

    Ok(parsed.results)
}

/// Map profile API base (`https://api.<region>.coralogix.com`) to the public gateway host
/// used by the Semantic Search REST API (`https://ng-api-http.<region>.coralogix.com`).
fn ng_api_http_base(endpoint: &str) -> String {
    let base = endpoint.trim_end_matches('/');
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

/// Build a TLS gRPC channel with required Coralogix metadata attached via interceptor.
async fn build_client(
    endpoint: &str,
    api_key: &str,
    team_id: &str,
) -> Result<
    SchemaStoreOllyLookupServiceClient<
        tonic::service::interceptor::InterceptedService<
            Channel,
            impl Fn(Request<()>) -> Result<Request<()>, tonic::Status>,
        >,
    >,
> {
    let channel = build_channel(endpoint).await?;

    let api_key_val: MetadataValue<_> = format!("Bearer {api_key}")
        .parse()
        .context("Invalid API key for gRPC metadata")?;
    let team_id_val: MetadataValue<_> = team_id
        .parse()
        .context("Invalid team_id for gRPC metadata")?;
    let client_id_val: MetadataValue<_> =
        CLIENT_ID.parse().expect("static CLIENT_ID is always valid");

    let client = SchemaStoreOllyLookupServiceClient::with_interceptor(
        channel,
        move |mut req: Request<()>| {
            req.metadata_mut()
                .insert("authorization", api_key_val.clone());
            req.metadata_mut()
                .insert("cgx-team-id", team_id_val.clone());
            req.metadata_mut()
                .insert("client_id", client_id_val.clone());
            Ok(req)
        },
    );

    Ok(client)
}

/// Build an authenticated TLS channel for the given Coralogix base URL.
async fn build_channel(endpoint: &str) -> Result<Channel> {
    let base = endpoint.trim_end_matches('/');
    let tls = ClientTlsConfig::new().with_webpki_roots();
    let channel = Channel::from_shared(base.to_string())
        .with_context(|| format!("Invalid gRPC endpoint: {base}"))?
        .tls_config(tls)
        .context("Failed to configure TLS for gRPC channel")?
        .connect()
        .await
        .context("Failed to connect to SchemaStore gRPC service")?;
    Ok(channel)
}

/// Convert a path_array like `["$d", "http", "status"]` into `"$d.http.status"`.
fn serialize_path_for_query(path_array: &[String]) -> String {
    path_array.join(".")
}
