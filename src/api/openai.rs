use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const EMBEDDING_MODEL: &str = "text-embedding-3-small";
const OPENAI_EMBEDDINGS_URL: &str = "https://api.openai.com/v1/embeddings";

#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

/// Generate an embedding vector for `text` using OpenAI's `text-embedding-3-small` model.
pub async fn generate_embedding(text: &str, api_key: &str) -> Result<Vec<f32>> {
    let client = reqwest::Client::new();
    let req = EmbeddingRequest {
        model: EMBEDDING_MODEL,
        input: text,
    };

    let resp = client
        .post(OPENAI_EMBEDDINGS_URL)
        .bearer_auth(api_key)
        .json(&req)
        .send()
        .await
        .context("Failed to call OpenAI embeddings API")?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI API error (HTTP {status}): {body}");
    }

    let body: EmbeddingResponse = resp
        .json()
        .await
        .context("Failed to parse OpenAI embeddings response")?;

    body.data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .context("OpenAI embeddings response contained no data")
}
