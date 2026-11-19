//! The send-your-data key the sender uses, and where it sends to: fetched
//! from the onboarding service's open route and cached next to the spool.
//!
//! The key is public (the route needs no auth) and belongs to a team used
//! only for CLI telemetry. It is still never printed: [`TelemetryKey`] has no
//! `Debug`.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A cached key is used this long before it is fetched again. A rotated key
/// does not wait for it: ingestion answers 401 and the sender fetches at once.
const TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// The domains Coralogix ingress hosts live under.
const DOMAINS: [&str; 5] = [
    "coralogix.com",
    "coralogix.us",
    "coralogix.in",
    "coralogix.net",
    "coralogixsg.com",
];

/// The route's answer: `{"key": "...", "host_url": "https://ingress..."}`.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryKey {
    key: String,
    host_url: String,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    #[serde(flatten)]
    key: TelemetryKey,
    /// Seconds since 1970.
    fetched_at: u64,
}

impl TelemetryKey {
    /// `self` when the key can go in a header and the host is one of ours.
    /// The service stores both as typed in by hand and checks neither.
    fn checked(self) -> Option<Self> {
        let key_ok = !self.key.is_empty() && self.key.bytes().all(|b| b.is_ascii_graphic());
        (key_ok && host_allowed(&self.host_url, cfg!(debug_assertions))).then_some(self)
    }

    pub fn bearer(&self) -> &str {
        &self.key
    }

    /// The OTLP/HTTP logs URL on the ingress host.
    pub fn logs_url(&self) -> String {
        format!("{}/v1/logs", self.host_url.trim_end_matches('/'))
    }
}

/// `https://` on a Coralogix domain, with nothing but the host. With
/// `loopback` (debug builds), also `http://` on this machine, for tests.
fn host_allowed(host_url: &str, loopback: bool) -> bool {
    let Ok(url) = url::Url::parse(host_url) else {
        return false;
    };
    let bare = url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    if !bare {
        return false;
    }
    match (url.scheme(), url.host()) {
        ("https", Some(url::Host::Domain(host))) => DOMAINS.iter().any(|domain| {
            // On a dot boundary: `evilcoralogix.com` is not ours.
            host == *domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|rest| rest.ends_with('.'))
        }),
        ("http", Some(url::Host::Ipv4(ip))) => loopback && ip.is_loopback(),
        _ => false,
    }
}

fn now_secs() -> u64 {
    super::spool::now_nanos() / 1_000_000_000
}

/// The cached key, if there is a valid one younger than [`TTL`].
pub fn load(cache: &Path) -> Option<TelemetryKey> {
    let cached: Cached = serde_json::from_str(&fs::read_to_string(cache).ok()?).ok()?;
    // A fetch time in the future is not trusted.
    let age = now_secs().checked_sub(cached.fetched_at)?;
    (age < TTL.as_secs()).then_some(cached.key)?.checked()
}

pub fn save(cache: &Path, key: &TelemetryKey) {
    let cached = Cached {
        key: key.clone(),
        fetched_at: now_secs(),
    };
    if let Ok(json) = serde_json::to_string(&cached) {
        let _ = fs::write(cache, json);
    }
}

/// Ask the route for the key. `None` for anything but a 200 with a usable
/// key and host: the route is not deployed (404), has no key yet (503), is
/// rate limited for all callers together (429), or is unreachable.
pub async fn fetch(client: &reqwest::Client, url: &str) -> Option<TelemetryKey> {
    let response = client.get(url).send().await.ok()?;
    if response.status() != reqwest::StatusCode::OK {
        return None;
    }
    response.json::<TelemetryKey>().await.ok()?.checked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(host_url: &str) -> TelemetryKey {
        TelemetryKey {
            key: "cxtp_test".into(),
            host_url: host_url.into(),
        }
    }

    #[test]
    fn only_https_coralogix_hosts_are_allowed() {
        for ok in [
            "https://ingress.eu2.coralogix.com",
            "https://ingress.eu2.coralogix.com/",
            "https://ingress.eu2.coralogix.com:443",
            "https://ingress.coralogix.us",
            "https://ingress.coralogix.in",
            "https://ingress.coralogixsg.com",
            "https://ingress.staging.coralogix.net",
            "https://INGRESS.EU2.CORALOGIX.COM",
        ] {
            assert!(host_allowed(ok, false), "{ok}");
        }
        for bad in [
            "http://ingress.eu2.coralogix.com",
            "https://evilcoralogix.com",
            "https://coralogix.com.evil.io",
            "https://ingress.eu2.coralogix.com/other/path",
            "https://ingress.eu2.coralogix.com?x=1",
            "https://user:pass@ingress.eu2.coralogix.com",
            "https://ingress.eu2.coralogix.com@evil.io",
            "https://127.0.0.1",
            "http://127.0.0.1:8080",
            "ingress.eu2.coralogix.com",
            "",
        ] {
            assert!(!host_allowed(bad, false), "{bad}");
        }
    }

    #[test]
    fn loopback_http_is_for_debug_builds_only() {
        assert!(host_allowed("http://127.0.0.1:8080", true));
        assert!(!host_allowed("http://10.0.0.1:8080", true));
        assert!(!host_allowed("http://evil.example.com", true));
    }

    #[test]
    fn a_key_that_cannot_be_a_header_is_refused() {
        let with = |k: &str| TelemetryKey {
            key: k.into(),
            host_url: "https://ingress.eu2.coralogix.com".into(),
        };
        assert!(with("cxtp_abc-123").checked().is_some());
        assert!(with("").checked().is_none());
        assert!(with("a b").checked().is_none());
        assert!(with("a\r\nX-Evil: 1").checked().is_none());
    }

    #[test]
    fn logs_url_is_on_the_ingress_host() {
        let url = "https://ingress.eu2.coralogix.com/v1/logs";
        assert_eq!(key("https://ingress.eu2.coralogix.com").logs_url(), url);
        assert_eq!(key("https://ingress.eu2.coralogix.com/").logs_url(), url);
    }

    #[test]
    fn cache_round_trips_and_expires() {
        let dir = std::env::temp_dir().join(format!("cx_key_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("telemetry-key.json");
        assert!(load(&cache).is_none(), "no file");

        let fresh = key("https://ingress.eu2.coralogix.com");
        save(&cache, &fresh);
        assert!(load(&cache) == Some(fresh.clone()));

        let write = |fetched_at: u64, host: &str| {
            let json = serde_json::json!({
                "key": "cxtp_test", "host_url": host, "fetched_at": fetched_at
            });
            fs::write(&cache, json.to_string()).unwrap();
        };
        write(now_secs() - TTL.as_secs() - 1, &fresh.host_url);
        assert!(load(&cache).is_none(), "older than the TTL");
        write(now_secs() + 3600, &fresh.host_url);
        assert!(load(&cache).is_none(), "fetched in the future");
        write(now_secs(), "https://evil.example.com");
        assert!(load(&cache).is_none(), "an edited host");
        fs::write(&cache, "not json").unwrap();
        assert!(load(&cache).is_none(), "corrupt");
        let _ = fs::remove_dir_all(dir);
    }
}
