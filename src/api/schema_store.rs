use anyhow::{Context, Result};
use serde::Serialize;
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
    SemanticMetricLookupRequest,
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

/// One result row returned by the semantic metric lookup.
#[derive(Debug, Serialize)]
pub struct SemanticMetricResult {
    pub metric_name: String,
    pub description: String,
    pub metric_type: String,
    pub metric_suffixes: Vec<String>,
    /// Semantic similarity (higher is more similar, range 0–1)
    pub similarity: f64,
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

/// Perform a semantic metric lookup against the SchemaStore gRPC service.
///
/// * `endpoint`       — Base API URL (e.g. `https://api.eu2.coralogix.com`)
/// * `api_key`        — Coralogix API key (Bearer token)
/// * `team_id`        — Coralogix team / company ID (sent as `cgx-team-id`)
/// * `openai_api_key` — OpenAI API key for generating the embedding
/// * `text`           — Free-text description to embed and search
/// * `limit`          — Maximum number of results
pub async fn semantic_metric_lookup(
    endpoint: &str,
    api_key: &str,
    team_id: &str,
    openai_api_key: &str,
    text: &str,
    limit: u32,
) -> Result<Vec<SemanticMetricResult>> {
    let company_id: u32 = team_id
        .parse()
        .with_context(|| format!("cgx-team-id must be a numeric company ID, got: {team_id}"))?;

    let embedding_values = generate_embedding(text, openai_api_key)
        .await
        .context("Failed to generate OpenAI embedding")?;

    let mut client = build_client(endpoint, api_key, team_id).await?;

    let request = SemanticMetricLookupRequest {
        company_id,
        embedding: Some(Embedding {
            embedding: Some(EmbeddingOneof::V1(EmbeddingV1 {
                values: embedding_values,
            })),
        }),
        limit,
    };

    let response = client
        .semantic_metric_lookup(request)
        .await
        .context("SchemaStore metric gRPC call failed")?;

    let results = response
        .into_inner()
        .results
        .into_iter()
        .map(|r| SemanticMetricResult {
            metric_name: r.metric_name,
            description: r.description,
            metric_type: r.metric_type,
            metric_suffixes: r.metric_suffixes,
            // The API stores cosine distance; invert to get similarity.
            similarity: (1.0 - r.similarity_score as f64).clamp(0.0, 1.0),
        })
        .collect();

    Ok(results)
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
    let client_id_val: MetadataValue<_> = CLIENT_ID
        .parse()
        .expect("static CLIENT_ID is always valid");

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
