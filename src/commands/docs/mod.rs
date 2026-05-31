//! Search and fetch official Coralogix product documentation (public llms.txt index).

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tokio::sync::Mutex;
use url::Url;

use crate::config::OutputFormat;
use crate::render;

pub const DOCS_LLMS_INDEX_URL: &str = "https://coralogix.com/docs/llms.txt";
const DOCS_HOST: &str = "coralogix.com";
const INDEX_TTL: Duration = Duration::from_secs(3600);

static INDEX_CACHE: LazyLock<Mutex<Option<CachedIndex>>> = LazyLock::new(|| Mutex::new(None));

struct CachedIndex {
    loaded_at: Instant,
    entries: Vec<(String, String)>,
}

#[derive(Debug, Serialize)]
struct SearchHit {
    rank: usize,
    title: String,
    url: String,
}

#[derive(Debug, Serialize)]
struct FetchDocResult {
    url: String,
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
        "https://coralogix.com/docs".to_string()
    } else {
        url
    }
}

/// Map a page URL to the markdown fetch URL (`.../index.md`).
pub fn md_url(url: &str) -> String {
    let page = page_url(url);
    if page.ends_with(".md") {
        page
    } else {
        format!("{page}/index.md")
    }
}

fn is_coralogix_docs_url(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.contains(DOCS_HOST)))
        .unwrap_or(false)
}

/// Parse `- [Title](https://...)` lines from llms.txt.
pub fn parse_index(text: &str) -> Result<Vec<(String, String)>> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let Some((title, raw_url)) = parse_link_line(line) else {
            continue;
        };
        let page = page_url(&raw_url);
        if is_coralogix_docs_url(&page) {
            entries.push((title, page));
        }
    }
    if entries.is_empty() {
        bail!("docs index is empty");
    }
    Ok(entries)
}

fn parse_link_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    let rest = line.strip_prefix("- [")?;
    let (title, rest) = rest.split_once("](")?;
    let url = rest.strip_suffix(')')?;
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    Some((title.to_string(), url.to_string()))
}

/// Score a query against a doc title and URL (same heuristics as ws-ai-mcp).
pub fn score_query(query: &str, title: &str, url: &str) -> i32 {
    let query = query.to_lowercase();
    let title = title.to_lowercase();
    let url = url.to_lowercase();
    if let Some(pos) = title.find(&query) {
        return 100 - pos as i32;
    }
    if url.contains(&query) {
        return 50;
    }
    let words: Vec<_> = query.split_whitespace().filter(|w| w.len() > 2).collect();
    if words.is_empty() {
        return 0;
    }
    words
        .iter()
        .filter(|w| title.contains(*w) || url.contains(*w))
        .count() as i32
        * 10
}

async fn fetch_text(url: &str) -> Result<String> {
    let client = reqwest::Client::new();
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

async fn load_index() -> Result<Vec<(String, String)>> {
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
    let mut ranked: Vec<(String, String, i32)> = index
        .into_iter()
        .map(|(title, url)| {
            let score = score_query(query, &title, &url);
            (title, url, score)
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });

    let hits: Vec<(String, String)> = ranked
        .into_iter()
        .filter(|(_, _, score)| *score > 0)
        .take(limit)
        .map(|(title, url, _)| (title, url))
        .collect();

    if hits.is_empty() {
        bail!("no docs matched {query:?}");
    }

    let rows: Vec<SearchHit> = hits
        .iter()
        .enumerate()
        .map(|(i, (title, url))| SearchHit {
            rank: i + 1,
            title: title.clone(),
            url: url.clone(),
        })
        .collect();

    match output {
        OutputFormat::Text => {
            for hit in &rows {
                println!("{}. {} — {}", hit.rank, hit.title, hit.url);
            }
        }
        OutputFormat::Json => {
            let json_rows: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .collect();
            render::render_json(&json_rows)?;
        }
        OutputFormat::Agents => {
            let values: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .collect();
            render::render_agents(&values)?;
        }
    }

    Ok(())
}

pub async fn run_fetch(url: &str, output: OutputFormat) -> Result<()> {
    let url = url.trim();
    if url.is_empty() {
        bail!("url is required");
    }
    let md = md_url(url);
    let body = fetch_text(&md).await?;
    let result = FetchDocResult {
        url: page_url(url),
        body: body.trim().to_string(),
    };

    match output {
        OutputFormat::Text => {
            println!("URL: {}\n", result.url);
            println!("{}", result.body);
        }
        OutputFormat::Json => {
            render::render_json_auto(&[serde_json::to_value(&result)?])?;
        }
        OutputFormat::Agents => {
            render::render_agents(&[serde_json::to_value(&result)?])?;
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

    #[test]
    fn parse_index_extracts_coralogix_links() {
        let entries = parse_index(SAMPLE_INDEX).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].1.contains("coralogix.com"));
    }

    #[test]
    fn page_url_strips_md_and_index() {
        assert_eq!(
            page_url("https://coralogix.com/docs/foo/index.md"),
            "https://coralogix.com/docs/foo"
        );
    }

    #[test]
    fn md_url_appends_index_md() {
        assert_eq!(
            md_url("https://coralogix.com/docs/foo/"),
            "https://coralogix.com/docs/foo/index.md"
        );
    }

    #[test]
    fn score_query_prefers_title_match() {
        let s = score_query(
            "explore spans",
            "Explore spans",
            "https://coralogix.com/docs/spans/",
        );
        assert!(s >= 90);
    }

    #[test]
    fn score_query_zero_for_unrelated() {
        let s = score_query(
            "zzzznotfound",
            "API keys",
            "https://coralogix.com/docs/api-keys/",
        );
        assert_eq!(s, 0);
    }
}
