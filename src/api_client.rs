use std::time::Duration;

use reqwest::{header, Client, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{CxError, Result};
use crate::safety::AGENT_ENV_VARS;

fn cli_user_agent(agent_env_var: Option<&str>) -> String {
    match agent_env_var {
        Some(agent_env_var) => format!("cx-cli/{}", agent_env_var.to_ascii_lowercase()),
        None => "cx-cli/direct".to_string(),
    }
}

/// Thin wrapper around reqwest::Client pre-configured with Coralogix auth.
#[derive(Clone)]
pub struct CxClient {
    inner: Client,
    endpoint: String,
}

impl CxClient {
    pub fn new(endpoint: impl Into<String>, api_key: &str) -> Result<Self> {
        Self::with_timeout(endpoint, api_key, None)
    }

    /// Builds a client with an optional deadline for each HTTP request.
    pub fn with_timeout(
        endpoint: impl Into<String>,
        api_key: &str,
        timeout: Option<Duration>,
    ) -> Result<Self> {
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
        headers.insert(
            header::HeaderName::from_static("x-cx-sdk-version"),
            header::HeaderValue::from_static(concat!("cx-cli-", env!("CARGO_PKG_VERSION"))),
        );

        let agent_env_var = AGENT_ENV_VARS
            .iter()
            .find(|var| std::env::var(var).is_ok())
            .copied();
        let mut builder = Client::builder()
            .default_headers(headers)
            .user_agent(cli_user_agent(agent_env_var));
        if let Some(timeout) = timeout {
            builder = builder.timeout(timeout);
        }
        let inner = builder.build()?;

        Ok(Self {
            inner,
            endpoint: normalize_endpoint(&endpoint.into()),
        })
    }

    /// The normalized base endpoint this client sends requests to (no trailing
    /// slash). Used for diagnostics — e.g. naming the endpoint in the
    /// health-check error when a region/URL looks wrong.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
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

    /// GET with optional query params, return the raw response text.
    pub async fn get_raw(
        &self,
        path: &str,
        params: &[(&str, &str)],
        headers: &[(&str, &str)],
    ) -> Result<String> {
        let url = format!("{}{path}", self.endpoint);
        let mut req = self.inner.get(&url).query(params);
        for (key, value) in headers {
            req = req.header(*key, *value);
        }
        let resp = req.send().await?;
        self.checked_text(resp).await
    }

    /// POST JSON body, deserialize response into T.
    pub async fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        self.post_with_headers(path, body, &[]).await
    }

    /// POST JSON body with extra headers, deserialize response into T.
    pub async fn post_with_headers<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &Value,
        headers: &[(&str, &str)],
    ) -> Result<T> {
        let url = format!("{}{path}", self.endpoint);
        let mut req = self.inner.post(&url).json(body);
        for (key, value) in headers {
            req = req.header(*key, *value);
        }
        let resp = req.send().await?;
        let text = self.checked_text(resp).await?;
        Ok(serde_json::from_str(&text)?)
    }

    /// PUT JSON body, deserialize response into T.
    pub async fn put<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        let url = format!("{}{path}", self.endpoint);
        let resp = self.inner.put(&url).json(body).send().await?;
        let text = self.checked_text(resp).await?;
        Ok(serde_json::from_str(&text)?)
    }

    /// DELETE a resource, deserialize response into T.
    /// Falls back to deserializing `{}` when the response body is empty.
    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{path}", self.endpoint);
        let resp = self.inner.delete(&url).send().await?;
        let text = self.checked_text(resp).await?;
        let json = if text.trim().is_empty() { "{}" } else { &text };
        Ok(serde_json::from_str(json)?)
    }

    /// DELETE with a JSON body, deserialize response into T.
    /// Falls back to deserializing `{}` when the response body is empty.
    pub async fn delete_with_body<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<T> {
        let url = format!("{}{path}", self.endpoint);
        let resp = self.inner.delete(&url).json(body).send().await?;
        let text = self.checked_text(resp).await?;
        let json = if text.trim().is_empty() { "{}" } else { &text };
        Ok(serde_json::from_str(json)?)
    }

    /// PATCH JSON body, deserialize response into T.
    pub async fn patch<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        let url = format!("{}{path}", self.endpoint);
        let resp = self.inner.patch(&url).json(body).send().await?;
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
        if status.is_success() {
            return Ok(resp.text().await?);
        }

        let code = status.as_u16();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body = resp.text().await.unwrap_or_default();
        let detail = extract_error_detail(&body);

        match status {
            StatusCode::UNAUTHORIZED => Err(CxError::Auth(match detail {
                Some(d) => format!("{d}. Run `cx profiles add` to update credentials."),
                None => "Invalid or expired API key. Run `cx profiles add` to update credentials."
                    .into(),
            })),
            StatusCode::FORBIDDEN => Err(CxError::Permission(match detail {
                Some(d) => d,
                None => {
                    "You do not have permission for this operation. Check your API key's scopes."
                        .into()
                }
            })),
            StatusCode::TOO_MANY_REQUESTS => Err(CxError::Api {
                status: code,
                message: match (detail, retry_after) {
                    (Some(d), Some(secs)) => format!("{d}. Retry after {secs} seconds."),
                    (Some(d), None) => d,
                    (None, Some(secs)) => {
                        format!("Rate limited by the API. Retry after {secs} seconds.")
                    }
                    (None, None) => "Rate limited by the API. Wait and retry.".into(),
                },
            }),
            _ => Err(CxError::Api {
                status: code,
                message: detail.unwrap_or(body),
            }),
        }
    }
}

/// Normalize a base endpoint so URL joining always produces a single `/`.
///
/// All request paths in this crate begin with a leading `/`, and URLs are
/// built with `format!("{endpoint}{path}")`. A trailing slash on the endpoint
/// (common for custom/overridden endpoints) would otherwise yield a `//` in the
/// path, which some Coralogix proxies reject (e.g. the metrics endpoint returns
/// 404 for `//metrics/api/v1/...`). Trimming trailing slashes here fixes this
/// for every command at a single chokepoint.
fn normalize_endpoint(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// Extract a human-readable error detail from a response body.
///
/// Tries common JSON error shapes in order:
/// 1. `{"message": "..."}`
/// 2. `{"error": "..."}` (string)
/// 3. `{"error": {"message": "..."}}`
/// 4. `{"detail": "..."}`
///
/// Returns `None` when the body is not JSON, none of the fields are present,
/// or every candidate is an empty string.
pub(crate) fn extract_error_detail(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let non_empty = |val: &Value| val.as_str().filter(|s| !s.is_empty()).map(String::from);
    non_empty(&v["message"])
        .or_else(|| non_empty(&v["error"]))
        .or_else(|| non_empty(&v["error"]["message"]))
        .or_else(|| non_empty(&v["detail"]))
}

#[cfg(test)]
mod tests {
    use super::{cli_user_agent, extract_error_detail, normalize_endpoint};

    #[test]
    fn user_agent_uses_detected_agent_environment_variable() {
        assert_eq!(cli_user_agent(Some("CURSOR_AGENT")), "cx-cli/cursor_agent");
        assert_eq!(cli_user_agent(Some("CLAUDE_CODE")), "cx-cli/claude_code");
        assert_eq!(cli_user_agent(None), "cx-cli/direct");
    }

    #[test]
    fn trims_single_trailing_slash() {
        assert_eq!(
            normalize_endpoint("https://api.eu1.coralogix.com/"),
            "https://api.eu1.coralogix.com"
        );
    }

    #[test]
    fn trims_multiple_trailing_slashes() {
        assert_eq!(
            normalize_endpoint("https://api.eu1.coralogix.com///"),
            "https://api.eu1.coralogix.com"
        );
    }

    #[test]
    fn leaves_endpoint_without_trailing_slash_unchanged() {
        assert_eq!(
            normalize_endpoint("https://api.eu1.coralogix.com"),
            "https://api.eu1.coralogix.com"
        );
    }

    #[test]
    fn prefers_top_level_message() {
        let body = r#"{"message":"primary","error":"secondary","detail":"tertiary"}"#;
        assert_eq!(extract_error_detail(body).as_deref(), Some("primary"));
    }

    #[test]
    fn falls_back_to_error_string() {
        let body = r#"{"error":"Token expired"}"#;
        assert_eq!(extract_error_detail(body).as_deref(), Some("Token expired"));
    }

    #[test]
    fn falls_back_to_nested_error_message() {
        let body = r#"{"error":{"message":"User lacks role"}}"#;
        assert_eq!(
            extract_error_detail(body).as_deref(),
            Some("User lacks role")
        );
    }

    #[test]
    fn falls_back_to_detail() {
        let body = r#"{"detail":"resource not found"}"#;
        assert_eq!(
            extract_error_detail(body).as_deref(),
            Some("resource not found")
        );
    }

    #[test]
    fn empty_strings_are_skipped() {
        let body = r#"{"message":"","error":"actual"}"#;
        assert_eq!(extract_error_detail(body).as_deref(), Some("actual"));
    }

    #[test]
    fn returns_none_for_non_json() {
        assert_eq!(extract_error_detail("plain text"), None);
        assert_eq!(extract_error_detail(""), None);
    }

    #[test]
    fn returns_none_when_no_recognized_fields() {
        let body = r#"{"code":42,"trace_id":"abc"}"#;
        assert_eq!(extract_error_detail(body), None);
    }

    #[test]
    fn returns_none_when_error_object_has_no_message() {
        let body = r#"{"error":{"code":"E_BAD"}}"#;
        assert_eq!(extract_error_detail(body), None);
    }
}
