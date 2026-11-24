use std::env;
use std::fmt::Write;
use std::path::PathBuf;

use crate::cli_otel::{header, Url};
use reqwest::StatusCode;
use sha2::{Digest, Sha256};
use url::Host;

const RELEASE_KEY_URL: &str = "https://api.eu2.coralogix.com/api/v2/cli-telemetry-key";
const MAX_KEY_BYTES: usize = 4096;

pub(super) enum Destination {
    Explicit { endpoint: Url, background: bool },
    Discovery { key_url: Url },
}

impl Destination {
    pub(super) fn resolve() -> Option<Self> {
        match env::var("CX_TELEMETRY_ENDPOINT") {
            Ok(value) => {
                let endpoint = http_url(value.trim(), true)?;
                let background = env::var("CX_TELEMETRY_DELIVERY")
                    .is_ok_and(|value| value.trim().eq_ignore_ascii_case("background"));
                Some(Self::Explicit {
                    endpoint,
                    background,
                })
            }
            Err(env::VarError::NotPresent) => {
                let key_url = match env::var("CX_TELEMETRY_KEY_URL") {
                    Ok(value) => http_url(value.trim(), cfg!(debug_assertions))?,
                    Err(env::VarError::NotPresent) if !cfg!(debug_assertions) => {
                        http_url(RELEASE_KEY_URL, false)?
                    }
                    _ => return None,
                };
                Some(Self::Discovery { key_url })
            }
            Err(env::VarError::NotUnicode(_)) => None,
        }
    }

    pub(super) fn url(&self) -> &Url {
        match self {
            Self::Explicit { endpoint, .. } => endpoint,
            Self::Discovery { key_url } => key_url,
        }
    }

    pub(super) fn is_background(&self) -> bool {
        match self {
            Self::Explicit { background, .. } => *background,
            Self::Discovery { .. } => true,
        }
    }

    pub(super) fn failure_logs_enabled(&self) -> bool {
        matches!(self, Self::Discovery { .. })
            || env::var("CX_TELEMETRY_FAILURE_LOGS").is_ok_and(|value| super::truthy(&value))
    }

    pub(super) fn spool_path(&self) -> Option<PathBuf> {
        let kind = match self {
            Self::Explicit { .. } => "endpoint",
            Self::Discovery { .. } => "discovery",
        };
        let digest = Sha256::digest(self.url().as_str());
        let mut partition = format!("{kind}-");
        for byte in &digest {
            write!(partition, "{byte:02x}").expect("writing to a String is infallible");
        }
        Some(
            crate::config::config_dir()
                .ok()?
                .join("telemetry")
                .join(partition),
        )
    }
}

pub(super) fn http_url(value: &str, allow_loopback_http: bool) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    if url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    match url.scheme() {
        "https" => Some(url),
        "http" if allow_loopback_http && is_loopback(&url) => Some(url),
        _ => None,
    }
}

pub(super) fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain("localhost")) => true,
        _ => false,
    }
}

pub(super) fn bearer(key: &str) -> Option<header::HeaderValue> {
    if key.is_empty()
        || key.len() > MAX_KEY_BYTES
        || !key.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return None;
    }
    let mut value = header::HeaderValue::from_str(&format!("Bearer {key}")).ok()?;
    value.set_sensitive(true);
    Some(value)
}

pub(super) fn explicit_headers() -> Option<header::HeaderMap> {
    let mut headers = header::HeaderMap::new();
    match env::var("CX_TELEMETRY_API_KEY") {
        Ok(key) if !key.is_empty() => {
            headers.insert(header::AUTHORIZATION, bearer(&key)?);
        }
        Ok(_) | Err(env::VarError::NotPresent) => {}
        Err(env::VarError::NotUnicode(_)) => return None,
    }
    Some(headers)
}

pub(super) fn retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}
