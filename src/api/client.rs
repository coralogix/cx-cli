use reqwest::{header, Client, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{CxError, Result};

/// Thin wrapper around reqwest::Client pre-configured with Coralogix auth.
#[derive(Clone)]
pub struct CxClient {
    inner: Client,
    endpoint: String,
}

impl CxClient {
    pub fn new(endpoint: impl Into<String>, api_key: &str) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {api_key}"))
                .map_err(|_| CxError::Auth("Invalid API key format".into()))?,
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        let inner = Client::builder()
            .default_headers(headers)
            .user_agent(concat!("cx-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;

        Ok(Self {
            inner,
            endpoint: endpoint.into(),
        })
    }

    /// POST JSON body, return the raw response text.
    /// Used for NDJSON / streaming endpoints (e.g. DataPrime query).
    pub async fn post_raw(&self, path: &str, body: &Value) -> Result<String> {
        let url = format!("{}{path}", self.endpoint);
        let resp = self.inner.post(&url).json(body).send().await?;
        self.checked_text(resp).await
    }

    /// GET with optional query params, deserialize response into T.
    pub async fn get<T: DeserializeOwned>(&self, path: &str, params: &[(&str, &str)]) -> Result<T> {
        let url = format!("{}{path}", self.endpoint);
        let resp = self.inner.get(&url).query(params).send().await?;
        let text = self.checked_text(resp).await?;
        Ok(serde_json::from_str(&text)?)
    }

    /// POST JSON body, deserialize response into T.
    pub async fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        let url = format!("{}{path}", self.endpoint);
        let resp = self.inner.post(&url).json(body).send().await?;
        let text = self.checked_text(resp).await?;
        Ok(serde_json::from_str(&text)?)
    }

    /// POST with query params but no body, deserialize response into T.
    /// Falls back to deserializing `{}` when the response body is empty
    /// (common for state-change endpoints that return 200/204 with no content).
    pub async fn post_empty<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T> {
        let url = format!("{}{path}", self.endpoint);
        let resp = self.inner.post(&url).query(params).send().await?;
        let text = self.checked_text(resp).await?;
        let json = if text.trim().is_empty() { "{}" } else { &text };
        Ok(serde_json::from_str(json)?)
    }

    /// Validate the HTTP status of a response and read the body as text.
    async fn checked_text(&self, resp: reqwest::Response) -> Result<String> {
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(CxError::Auth(
                "Invalid or expired API key. Run `cx configure` to update credentials.".into(),
            ));
        }
        if !status.is_success() {
            let code = status.as_u16();
            let body = resp.text().await.unwrap_or_default();
            let message = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v["message"].as_str().map(String::from))
                .unwrap_or(body);
            return Err(CxError::Api {
                status: code,
                message,
            });
        }
        Ok(resp.text().await?)
    }
}
