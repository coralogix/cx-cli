use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cli_otel::{command_failure_logs_endpoint, header, Url};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use url::Host;

use super::delivery::{bearer, http_url, is_loopback, retryable};

const TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_KEY_RESPONSE_BYTES: usize = 16 * 1024;
const CORALOGIX_DOMAINS: &[&str] = &[
    "coralogix.com",
    "coralogix.us",
    "coralogix.in",
    "coralogix.net",
    "coralogixsg.com",
];

// The send-your-data key is still sensitive.
#[derive(Deserialize, Serialize)]
struct KeyResponse {
    key: String,
    host_url: String,
}

impl KeyResponse {
    fn target(&self, key_url: &Url) -> Option<Target> {
        let allow_loopback = cfg!(debug_assertions) && is_loopback(key_url);
        let endpoint = ingress_endpoint(&self.host_url, allow_loopback)?;
        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, bearer(&self.key)?);
        Some(Target::new(endpoint, headers))
    }
}

#[derive(Deserialize, Serialize)]
struct CachedKey {
    #[serde(flatten)]
    data: KeyResponse,
    fetched_at: u64,
}

pub(super) struct Target {
    pub(super) endpoint: Url,
    pub(super) logs_endpoint: Option<Url>,
    pub(super) headers: header::HeaderMap,
}

impl Target {
    pub(super) fn new(endpoint: Url, headers: header::HeaderMap) -> Self {
        let logs_endpoint = command_failure_logs_endpoint(&endpoint);
        Self {
            endpoint,
            logs_endpoint,
            headers,
        }
    }
}

pub(super) enum FetchFailure {
    Temporary,
    Unavailable,
}

fn ingress_endpoint(host_url: &str, allow_loopback: bool) -> Option<Url> {
    // Check the original authority too, because a URL parser can normalize an
    // input path such as /one/.. into a bare host.
    let (_, authority) = host_url.split_once("://")?;
    let authority = authority.strip_suffix('/').unwrap_or(authority);
    if authority.is_empty()
        || authority.contains(['/', '\\', '@', '?', '#'])
        || authority
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return None;
    }
    let mut url = http_url(host_url, allow_loopback)?;
    let allowed = match (url.scheme(), url.host()) {
        ("https", Some(Host::Domain(host))) if url.port().is_none() => {
            CORALOGIX_DOMAINS.iter().any(|domain| {
                host == *domain
                    || host
                        .strip_suffix(domain)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            })
        }
        ("http", _) => allow_loopback && is_loopback(&url),
        _ => false,
    };
    if !allowed {
        return None;
    }
    url.set_path("/v1/traces");
    Some(url)
}

fn now() -> Option<u64> {
    Some(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs())
}

pub(super) fn load(cache: &Path, key_url: &Url) -> Option<Target> {
    let metadata = fs::symlink_metadata(cache).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_KEY_RESPONSE_BYTES as u64 {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return None;
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(cache)
        .ok()?
        .take(MAX_KEY_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_KEY_RESPONSE_BYTES {
        return None;
    }
    let cached: CachedKey = serde_json::from_slice(&bytes).ok()?;
    let age = now()?.checked_sub(cached.fetched_at)?;
    if age >= TTL.as_secs() {
        return None;
    }
    cached.data.target(key_url)
}

pub(super) async fn fetch(
    client: &Client,
    key_url: &Url,
    cache: &Path,
) -> Result<Target, FetchFailure> {
    if !super::consent_allows() {
        return Err(FetchFailure::Unavailable);
    }
    let mut response = client
        .get(key_url.clone())
        .send()
        .await
        .map_err(|_| FetchFailure::Temporary)?;
    if retryable(response.status()) {
        return Err(FetchFailure::Temporary);
    }
    if response.status() != StatusCode::OK
        || response
            .content_length()
            .is_some_and(|length| length > MAX_KEY_RESPONSE_BYTES as u64)
    {
        return Err(FetchFailure::Unavailable);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| FetchFailure::Temporary)?
    {
        if chunk.len() > MAX_KEY_RESPONSE_BYTES - bytes.len() {
            return Err(FetchFailure::Unavailable);
        }
        bytes.extend_from_slice(&chunk);
    }
    let data: KeyResponse =
        serde_json::from_slice(&bytes).map_err(|_| FetchFailure::Unavailable)?;
    let target = data.target(key_url).ok_or(FetchFailure::Unavailable)?;
    if super::consent_allows() {
        if let Some(fetched_at) = now() {
            if let Ok(bytes) = serde_json::to_vec(&CachedKey { data, fetched_at }) {
                let _ = crate::atomic_file::write(cache, &bytes);
            }
        }
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingress_requires_a_bare_approved_host() {
        for host in [
            "https://ingress.eu2.coralogix.com",
            "https://INGRESS.CORALOGIX.US:443/",
            "https://ingress.coralogix.in",
            "https://ingress.coralogixsg.com",
            "https://ingress.stg1.coralogix.net",
        ] {
            assert_eq!(ingress_endpoint(host, false).unwrap().path(), "/v1/traces");
        }
        for host in [
            "http://ingress.eu2.coralogix.com",
            "https://lookslikecoralogix.com",
            "https://coralogix.com.example.org",
            "https://user:pass@ingress.coralogix.com",
            "https://ingress.coralogix.com@evil.example.org",
            "https://ingress.coralogix.com:8443",
            "https://ingress.coralogix.com/target/..",
            "https://ingress.coralogix.com#",
            "https://ingress.coralogix.com?",
            "https://127.0.0.1",
            "http://127.0.0.1:8080",
        ] {
            assert!(ingress_endpoint(host, false).is_none(), "{host}");
        }
        assert!(ingress_endpoint("http://127.0.0.1:8080", true).is_some());
        assert!(ingress_endpoint("http://[::1]:8080", true).is_some());
        assert!(ingress_endpoint("http://192.168.1.1:8080", true).is_none());
    }
}
