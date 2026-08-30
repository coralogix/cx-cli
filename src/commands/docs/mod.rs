//! Search and fetch official Coralogix product documentation (public llms.txt index).

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use strsim::jaro;
use tokio::sync::Mutex;
use url::Url;

use crate::config::OutputFormat;
use crate::render;

pub const DOCS_LLMS_INDEX_URL: &str = "https://coralogix.com/docs/llms.txt";
const DOCS_HOST: &str = "coralogix.com";
const DOCS_BASE_URL: &str = "https://coralogix.com/docs";
const INDEX_TTL: Duration = Duration::from_secs(3600);
// The docs CDN rejects reqwest's default user agent. Keep the cx identity as
// a product token while using a curl-compatible leading token.
const DOCS_USER_AGENT: &str = concat!("curl/8.0 cx-cli/", env!("CARGO_PKG_VERSION"));
/// Minimum best-hit score (same threshold as ws-ai-mcp `search_docs`).
const MIN_FUZZY_SCORE: f64 = 0.2;

static INDEX_CACHE: LazyLock<Mutex<Option<CachedIndex>>> = LazyLock::new(|| Mutex::new(None));

struct CachedIndex {
    loaded_at: Instant,
    entries: Vec<(String, String, String)>,
}

#[derive(Debug, Serialize)]
struct SearchHit {
    rank: usize,
    title: String,
    suffix: String,
}

#[derive(Debug, Serialize)]
struct FetchDocResult {
    suffix: String,
    body: String,
}

/// Normalize a docs page URL (strip `.md`, trailing `/index`, trailing slash).
pub fn page_url(url: &str) -> String {
    let mut url = url.trim().trim_end_matches('/').to_string();
    if url.ends_with(".md") {
        url.truncate(url.len() - 3);
    }
    if url.ends_with("/index") {
        url.truncate(url.len() - 6);
    }
    if url.is_empty() {
        DOCS_BASE_URL.to_string()
    } else {
        url
    }
}

/// Path under `/docs/`, preserving the suffix supplied by the docs index.
pub fn docs_suffix(url: &str) -> String {
    let path = Url::parse(url.trim())
        .ok()
        .map(|u| u.path().trim_end_matches('/').to_string())
        .unwrap_or_default();
    let prefix = "/docs";
    let path = path.strip_prefix(prefix).unwrap_or(&path);
    path.trim_start_matches('/').to_string()
}

/// Validate and normalize a docs path suffix from `cx docs search`.
pub fn normalize_suffix(value: &str) -> Result<String> {
    if value.starts_with("http://") || value.starts_with("https://") {
        bail!("expected a path suffix from `cx docs search` (e.g. user-guides/foo)");
    }
    let mut suffix = value.trim().trim_start_matches('/').to_string();
    if let Some(rest) = suffix.strip_prefix("docs/") {
        suffix = rest.to_string();
    }
    Ok(suffix)
}

pub fn page_url_from_suffix(suffix: &str) -> Result<String> {
    Ok(format!("{DOCS_BASE_URL}/{}", normalize_suffix(suffix)?))
}

/// Return the page URL unchanged for fetching.
pub fn md_url(url: &str) -> String {
    url.trim().to_string()
}

fn is_coralogix_docs_url(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.contains(DOCS_HOST)))
        .unwrap_or(false)
}

/// Parse docs index links, keeping the legacy text used for fuzzy ranking.
pub fn parse_index(text: &str) -> Result<Vec<(String, String, String)>> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let Some((title, raw_url, ranking_url)) = parse_link_line(line) else {
            continue;
        };
        if is_coralogix_docs_url(&raw_url) {
            entries.push((title, docs_suffix(&raw_url), docs_suffix(&ranking_url)));
        }
    }
    if entries.is_empty() {
        bail!("docs index is empty");
    }
    Ok(entries)
}

fn parse_link_line(line: &str) -> Option<(String, String, String)> {
    let line = line.trim();
    let rest = line.strip_prefix("- [")?;
    let (title, rest) = rest.split_once("](")?;
    let (url, _) = rest.split_once(')')?;
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    Some((
        title.to_string(),
        url.to_string(),
        rest.strip_suffix(')').unwrap_or(rest).to_string(),
    ))
}

/// Fuzzy score for a query against title and path suffix (same approach as ws-ai-mcp).
pub fn fuzzy_score(query: &str, title: &str, suffix: &str) -> f64 {
    let q = query.to_lowercase();
    if q.trim().is_empty() {
        return 0.0;
    }
    let t = title.to_lowercase();
    let s = suffix.to_lowercase();
    (jaro(&q, &t) + jaro(&q, &s)) / 2.0
}

async fn fetch_text(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(DOCS_USER_AGENT)
        .build()
        .context("failed to build documentation HTTP client")?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to request {url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("HTTP {status} for {url}");
    }
    response
        .text()
        .await
        .with_context(|| format!("failed to read body from {url}"))
}

async fn load_index() -> Result<Vec<(String, String, String)>> {
    {
        let guard = INDEX_CACHE.lock().await;
        if let Some(cache) = guard.as_ref() {
            if cache.loaded_at.elapsed() < INDEX_TTL {
                return Ok(cache.entries.clone());
            }
        }
    }

    let text = fetch_text(DOCS_LLMS_INDEX_URL).await?;
    let entries = parse_index(&text)?;

    let mut guard = INDEX_CACHE.lock().await;
    *guard = Some(CachedIndex {
        loaded_at: Instant::now(),
        entries: entries.clone(),
    });
    Ok(entries)
}

pub async fn run_search(query: &str, limit: u32, output: OutputFormat) -> Result<()> {
    let query = query.trim();
    if query.is_empty() {
        bail!("query is required");
    }
    let limit = limit.clamp(1, 20) as usize;

    let index = load_index().await?;
    let mut ranked: Vec<(String, String, f64)> = index
        .into_iter()
        .map(|(title, suffix, ranking_suffix)| {
            let score = fuzzy_score(query, &title, &ranking_suffix);
            (title, suffix, score)
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });

    if ranked.is_empty() || ranked[0].2 < MIN_FUZZY_SCORE {
        bail!("no docs matched {query:?}");
    }

    let hits: Vec<(String, String)> = ranked
        .into_iter()
        .take(limit)
        .map(|(title, suffix, _)| (title, suffix))
        .collect();

    let rows: Vec<SearchHit> = hits
        .iter()
        .enumerate()
        .map(|(i, (title, suffix))| SearchHit {
            rank: i + 1,
            title: title.clone(),
            suffix: suffix.clone(),
        })
        .collect();

    match output {
        OutputFormat::Text => {
            for hit in &rows {
                println!("{}. {} — {}", hit.rank, hit.title, hit.suffix);
            }
        }
        OutputFormat::Json => {
            let json_rows: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .collect();
            render::render_json(&json_rows)?;
        }
        OutputFormat::Toon => {
            let values: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .collect();
            render::render_toon(&values)?;
        }
    }

    Ok(())
}

pub async fn run_fetch(suffix: &str, output: OutputFormat) -> Result<()> {
    let suffix = normalize_suffix(suffix)?;
    let md = md_url(&page_url_from_suffix(&suffix)?);
    let body = fetch_text(&md).await?;
    let result = FetchDocResult {
        suffix: suffix.clone(),
        body: body.trim().to_string(),
    };

    match output {
        OutputFormat::Text => {
            println!("Path: {}\n", result.suffix);
            println!("{}", result.body);
        }
        OutputFormat::Json => {
            render::render_json_auto(&[serde_json::to_value(&result)?])?;
        }
        OutputFormat::Toon => {
            render::render_toon(&[serde_json::to_value(&result)?])?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_INDEX: &str = r#"
# Docs
- [Explore spans](https://coralogix.com/docs/user-guides/data_exploration/spans/)
- [API keys](https://coralogix.com/docs/user-guides/account-management/api-keys/)
- [Other host](https://example.com/docs/page/)
"#;
    const INDEX_WITH_DESCRIPTION: &str = r#"
- [Search](https://coralogix.com/docs/search.md): Find product docs.
"#;

    #[test]
    fn parse_index_extracts_coralogix_suffixes() {
        let entries = parse_index(SAMPLE_INDEX).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1, "user-guides/data_exploration/spans");
        assert_eq!(entries[0].2, "user-guides/data_exploration/spans");
        assert!(entries[0].1.contains("user-guides"));
    }

    #[test]
    fn parse_index_keeps_legacy_ranking_text() {
        let entries = parse_index(INDEX_WITH_DESCRIPTION).unwrap();
        assert_eq!(entries[0].1, "search.md");
        assert!(entries[0].2.contains("Find%20product%20docs"));
    }

    #[test]
    fn docs_suffix_strips_docs_prefix() {
        assert_eq!(
            docs_suffix("https://coralogix.com/docs/foo/bar/"),
            "foo/bar"
        );
    }

    #[test]
    fn normalize_suffix_strips_leading_slash_and_docs_prefix() {
        assert_eq!(
            normalize_suffix("/docs/user-guides/foo").unwrap(),
            "user-guides/foo"
        );
    }

    #[test]
    fn normalize_suffix_rejects_url() {
        assert!(normalize_suffix("https://coralogix.com/docs/foo").is_err());
    }

    #[test]
    fn page_url_strips_md_and_index() {
        assert_eq!(
            page_url("https://coralogix.com/docs/foo/index.md"),
            "https://coralogix.com/docs/foo"
        );
    }

    #[test]
    fn md_url_preserves_page_url() {
        assert_eq!(
            md_url("https://coralogix.com/docs/foo/"),
            "https://coralogix.com/docs/foo/"
        );
    }

    #[test]
    fn fuzzy_score_high_for_close_match() {
        let s = fuzzy_score(
            "explore spans",
            "Explore spans",
            "user-guides/data_exploration/spans",
        );
        assert!(s >= MIN_FUZZY_SCORE);
    }

    #[test]
    fn fuzzy_score_low_for_unrelated() {
        let s = fuzzy_score(
            "zzzznotfound",
            "API keys",
            "user-guides/account-management/api-keys",
        );
        assert!(s < MIN_FUZZY_SCORE);
    }
}
