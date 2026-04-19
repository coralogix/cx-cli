use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use tabled::{Table, Tabled};
use toon_format::encode_default as toon_encode;

use crate::api::{
    client::CxClient,
    metrics::{MetricsApi, PromQueryInstantResponse, PromQueryRangeResponse},
    semantic_search::{semantic_metric_lookup, SemanticMetricResult},
};
use crate::config::OutputFormat;
use crate::execution::{fan_out, ExecutionTarget};
use crate::time::parse_timestamp;

// ── Text-output row types ─────────────────────────────────────────────────────

// Multi-profile row types (with Profile column)
#[derive(Tabled)]
struct RangeRow {
    #[tabled(rename = "Profile")]
    profile: String,
    #[tabled(rename = "Labels")]
    labels: String,
    #[tabled(rename = "Timestamp")]
    timestamp: String,
    #[tabled(rename = "Value")]
    value: String,
}

#[derive(Tabled)]
struct InstantRow {
    #[tabled(rename = "Profile")]
    profile: String,
    #[tabled(rename = "Labels")]
    labels: String,
    #[tabled(rename = "Value")]
    value: String,
}

#[derive(Tabled)]
struct MetricSearchRow {
    #[tabled(rename = "Profile")]
    profile: String,
    #[tabled(rename = "Metric name")]
    metric_name: String,
    #[tabled(rename = "Description")]
    description: String,
    #[tabled(rename = "Similarity")]
    similarity: String,
}

// Single-profile row types (without Profile column)
#[derive(Tabled)]
struct RangeRowSingle {
    #[tabled(rename = "Labels")]
    labels: String,
    #[tabled(rename = "Timestamp")]
    timestamp: String,
    #[tabled(rename = "Value")]
    value: String,
}

#[derive(Tabled)]
struct InstantRowSingle {
    #[tabled(rename = "Labels")]
    labels: String,
    #[tabled(rename = "Value")]
    value: String,
}

#[derive(Tabled)]
struct MetricSearchRowSingle {
    #[tabled(rename = "Metric name")]
    metric_name: String,
    #[tabled(rename = "Description")]
    description: String,
    #[tabled(rename = "Similarity")]
    similarity: String,
}

#[derive(Tabled)]
struct MetricNameRow {
    #[tabled(rename = "Profile")]
    profile: String,
    #[tabled(rename = "Metric name")]
    metric_name: String,
}

#[derive(Tabled)]
struct MetricNameRowSingle {
    #[tabled(rename = "Metric name")]
    metric_name: String,
}

#[derive(Tabled)]
struct LabelRow {
    #[tabled(rename = "Label")]
    label: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn format_labels(metric: &std::collections::HashMap<String, String>) -> String {
    let pairs = metric
        .iter()
        .map(|(k, v)| format!("{k}=\"{v}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{pairs}}}")
}

/// Wildcard pattern matching for metric name search.
/// `*` matches any sequence of characters (including empty).
/// Case-sensitive.
fn matches_pattern(name: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return name.contains(pattern);
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !name.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if i == parts.len() - 1 {
            return name[pos..].ends_with(part);
        } else {
            match name[pos..].find(part) {
                Some(idx) => pos += idx + part.len(),
                None => return false,
            }
        }
    }
    true
}

/// Build toon-ready flat JSON rows from instant samples.  Includes the profile
/// name as an extra column only when `include_profile` is true.
pub(crate) fn instant_samples_to_toon_rows(samples: &[Value], include_profile: bool) -> Vec<Value> {
    // Collect sorted metric label keys first, optionally append "profile", then "value".
    // Keeping "value" at the end mirrors the original ordering and is friendlier
    // to toon-format's tabular layout heuristic.
    let all_keys: Vec<String> = {
        let mut set = std::collections::BTreeSet::new();
        for s in samples {
            if let Some(metric) = s.get("metric").and_then(|m| m.as_object()) {
                for k in metric.keys() {
                    set.insert(k.clone());
                }
            }
        }
        let mut keys: Vec<String> = set.into_iter().collect();
        if include_profile {
            keys.push("profile".to_string());
        }
        keys.push("value".to_string());
        keys
    };

    samples
        .iter()
        .map(|s| {
            let mut obj = serde_json::Map::new();
            for k in &all_keys {
                let v = match k.as_str() {
                    "value" => s
                        .get("value")
                        .and_then(|arr| arr.get(1))
                        .cloned()
                        .unwrap_or(Value::Null),
                    "profile" => s.get("profile").cloned().unwrap_or(Value::Null),
                    _ => s
                        .get("metric")
                        .and_then(|m| m.get(k))
                        .cloned()
                        .unwrap_or_else(|| Value::String(String::new())),
                };
                obj.insert(k.clone(), v);
            }
            Value::Object(obj)
        })
        .collect()
}

// ── Normalise metric responses to optionally tagged JSON rows ─────────────────

/// Convert a `PromQueryInstantResponse` into flat JSON rows.
///
/// When `include_profile` is true: `{ "profile": "...", "metric": { labels }, "value": [ts, val] }`
/// When false: `{ "metric": { labels }, "value": [ts, val] }`
fn instant_response_to_rows(
    profile: &str,
    resp: PromQueryInstantResponse,
    include_profile: bool,
) -> Vec<Value> {
    resp.data
        .result
        .into_iter()
        .map(|s| {
            if include_profile {
                json!({
                    "profile": profile,
                    "metric": s.metric,
                    "value": s.value,
                })
            } else {
                json!({
                    "metric": s.metric,
                    "value": s.value,
                })
            }
        })
        .collect()
}

/// Convert a `PromQueryRangeResponse` into flat JSON rows.
///
/// When `include_profile` is true: `{ "profile": "...", "metric": { labels }, "values": [[ts, val], ...] }`
/// When false: `{ "metric": { labels }, "values": [[ts, val], ...] }`
fn range_response_to_rows(
    profile: &str,
    resp: PromQueryRangeResponse,
    include_profile: bool,
) -> Vec<Value> {
    resp.data
        .result
        .into_iter()
        .map(|s| {
            if include_profile {
                json!({
                    "profile": profile,
                    "metric": s.metric,
                    "values": s.values,
                })
            } else {
                json!({
                    "metric": s.metric,
                    "values": s.values,
                })
            }
        })
        .collect()
}

// ── Execute helpers ───────────────────────────────────────────────────────────

async fn execute_instant(
    target: Arc<ExecutionTarget>,
    expr: String,
    time_ts: Option<String>,
) -> Result<PromQueryInstantResponse> {
    let api = MetricsApi::new(&target.client);
    let resp = api.query(&expr, time_ts.as_deref()).await?;
    if resp.status != "success" {
        bail!("Query returned non-success status: {}", resp.status);
    }
    Ok(resp)
}

async fn execute_range(
    target: Arc<ExecutionTarget>,
    expr: String,
    start_ts: String,
    end_ts: String,
    step: String,
) -> Result<PromQueryRangeResponse> {
    let api = MetricsApi::new(&target.client);
    let resp = api.query_range(&expr, &start_ts, &end_ts, &step).await?;
    if resp.status != "success" {
        bail!("Query returned non-success status: {}", resp.status);
    }
    Ok(resp)
}

async fn execute_metric_names(client: &CxClient, pattern: &str) -> Result<Vec<String>> {
    let api = MetricsApi::new(client);
    let resp = api.metric_names().await?;
    if resp.status != "success" {
        bail!("Request returned non-success status: {}", resp.status);
    }
    Ok(resp
        .data
        .into_iter()
        .filter(|n| matches_pattern(n, pattern))
        .collect())
}

async fn execute_labels(client: &CxClient, metric: &str) -> Result<Vec<String>> {
    let api = MetricsApi::new(client);
    let resp = api.labels_for_metric(metric).await?;
    if resp.status != "success" {
        bail!("Request returned non-success status: {}", resp.status);
    }
    Ok(resp.data)
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_query(
    targets: &[Arc<ExecutionTarget>],
    expr: &str,
    time: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Querying metrics (instant)...".dimmed());

    let include_profile = targets.len() > 1;
    let time_ts: Option<String> = time.map(parse_timestamp).transpose()?;
    let expr = expr.to_string();

    let per_profile = fan_out(targets, |t| {
        let e = expr.clone();
        let ts = time_ts.clone();
        async move { execute_instant(t, e, ts).await }
    })
    .await;

    // Merge: convert each profile's response to optionally tagged rows.
    let mut all_rows: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => all_rows.extend(instant_response_to_rows(&profile, resp, include_profile)),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&all_rows)?);
        }
        OutputFormat::Agents => {
            if all_rows.is_empty() {
                println!("[]");
                return Ok(());
            }
            let toon_rows = instant_samples_to_toon_rows(&all_rows, include_profile);
            let toon = toon_encode(&toon_rows)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_rows.is_empty() {
                println!("{}", "No results found.".yellow());
                return Ok(());
            }
            if include_profile {
                let rows: Vec<InstantRow> = all_rows
                    .iter()
                    .map(|s| {
                        let profile = s
                            .get("profile")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string();
                        let val = s
                            .get("value")
                            .and_then(|arr| arr.get(1))
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string();
                        let metric = s.get("metric").and_then(|m| m.as_object());
                        let labels = metric
                            .map(|m| {
                                let map: std::collections::HashMap<String, String> = m
                                    .iter()
                                    .filter_map(|(k, v)| {
                                        v.as_str().map(|s| (k.clone(), s.to_string()))
                                    })
                                    .collect();
                                format_labels(&map)
                            })
                            .unwrap_or_default();
                        InstantRow {
                            profile,
                            labels,
                            value: val,
                        }
                    })
                    .collect();
                println!("{}", Table::new(rows));
            } else {
                let rows: Vec<InstantRowSingle> = all_rows
                    .iter()
                    .map(|s| {
                        let val = s
                            .get("value")
                            .and_then(|arr| arr.get(1))
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string();
                        let metric = s.get("metric").and_then(|m| m.as_object());
                        let labels = metric
                            .map(|m| {
                                let map: std::collections::HashMap<String, String> = m
                                    .iter()
                                    .filter_map(|(k, v)| {
                                        v.as_str().map(|s| (k.clone(), s.to_string()))
                                    })
                                    .collect();
                                format_labels(&map)
                            })
                            .unwrap_or_default();
                        InstantRowSingle { labels, value: val }
                    })
                    .collect();
                println!("{}", Table::new(rows));
            }
        }
    }

    Ok(())
}

pub async fn run_query_range(
    targets: &[Arc<ExecutionTarget>],
    expr: &str,
    start: &str,
    end: &str,
    step: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Querying metrics (range)...".dimmed());

    let include_profile = targets.len() > 1;
    let start_ts = parse_timestamp(start)?;
    let end_ts = parse_timestamp(end)?;
    let expr = expr.to_string();
    let step = step.to_string();

    let per_profile = fan_out(targets, |t| {
        let e = expr.clone();
        let s = start_ts.clone();
        let en = end_ts.clone();
        let st = step.clone();
        async move { execute_range(t, e, s, en, st).await }
    })
    .await;

    let mut all_rows: Vec<Value> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(resp) => all_rows.extend(range_response_to_rows(&profile, resp, include_profile)),
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json | OutputFormat::Agents => {
            println!("{}", serde_json::to_string_pretty(&all_rows)?);
        }
        OutputFormat::Text => {
            if all_rows.is_empty() {
                println!("{}", "No results found.".yellow());
                return Ok(());
            }
            if include_profile {
                let mut rows: Vec<RangeRow> = Vec::new();
                for series in &all_rows {
                    let profile = series
                        .get("profile")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string();
                    let metric = series.get("metric").and_then(|m| m.as_object());
                    let labels = metric
                        .map(|m| {
                            let map: std::collections::HashMap<String, String> = m
                                .iter()
                                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                                .collect();
                            format_labels(&map)
                        })
                        .unwrap_or_default();
                    if let Some(values) = series.get("values").and_then(|v| v.as_array()) {
                        for point in values {
                            let ts = point.get(0).map(|v| v.to_string()).unwrap_or_default();
                            let val = point
                                .get(1)
                                .and_then(|v| v.as_str())
                                .unwrap_or("-")
                                .to_string();
                            rows.push(RangeRow {
                                profile: profile.clone(),
                                labels: labels.clone(),
                                timestamp: ts,
                                value: val,
                            });
                        }
                    }
                }
                println!("{}", Table::new(rows));
            } else {
                let mut rows: Vec<RangeRowSingle> = Vec::new();
                for series in &all_rows {
                    let metric = series.get("metric").and_then(|m| m.as_object());
                    let labels = metric
                        .map(|m| {
                            let map: std::collections::HashMap<String, String> = m
                                .iter()
                                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                                .collect();
                            format_labels(&map)
                        })
                        .unwrap_or_default();
                    if let Some(values) = series.get("values").and_then(|v| v.as_array()) {
                        for point in values {
                            let ts = point.get(0).map(|v| v.to_string()).unwrap_or_default();
                            let val = point
                                .get(1)
                                .and_then(|v| v.as_str())
                                .unwrap_or("-")
                                .to_string();
                            rows.push(RangeRowSingle {
                                labels: labels.clone(),
                                timestamp: ts,
                                value: val,
                            });
                        }
                    }
                }
                println!("{}", Table::new(rows));
            }
        }
    }

    Ok(())
}

pub async fn run_search(
    targets: &[Arc<ExecutionTarget>],
    name_pattern: Option<&str>,
    description_pattern: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let include_profile = targets.len() > 1;

    if let Some(desc) = description_pattern {
        eprintln!("{}", format!("Searching metrics for: {desc:?}…").dimmed());

        // Semantic search: fan out per target.
        let desc = desc.to_string();
        let per_profile = fan_out(targets, |t| {
            let d = desc.clone();
            async move {
                semantic_metric_lookup(&t.client, &d, 5)
                    .await
                    .map_err(Into::into)
            }
        })
        .await;

        let mut all_results: Vec<(String, SemanticMetricResult)> = Vec::new();
        for (profile, result) in per_profile {
            match result {
                Ok(results) => {
                    for r in results {
                        all_results.push((profile.clone(), r));
                    }
                }
                Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
            }
        }

        match output {
            OutputFormat::Json | OutputFormat::Agents => {
                let json_rows: Vec<Value> = all_results
                    .iter()
                    .map(|(profile, r)| {
                        let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
                        if include_profile {
                            if let Value::Object(ref mut m) = v {
                                m.insert("profile".to_string(), Value::String(profile.clone()));
                            }
                        }
                        v
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&json_rows)?);
            }
            OutputFormat::Text => {
                if all_results.is_empty() {
                    println!("{}", "No matching metrics found.".yellow());
                    return Ok(());
                }
                if include_profile {
                    let rows: Vec<MetricSearchRow> = all_results
                        .iter()
                        .map(|(profile, r)| MetricSearchRow {
                            profile: profile.clone(),
                            metric_name: r.metric_name.clone(),
                            description: r.description.clone(),
                            similarity: format!("{:.3}", r.similarity_score),
                        })
                        .collect();
                    println!("{}", Table::new(rows));
                } else {
                    let rows: Vec<MetricSearchRowSingle> = all_results
                        .iter()
                        .map(|(_, r)| MetricSearchRowSingle {
                            metric_name: r.metric_name.clone(),
                            description: r.description.clone(),
                            similarity: format!("{:.3}", r.similarity_score),
                        })
                        .collect();
                    println!("{}", Table::new(rows));
                }
            }
        }
        return Ok(());
    }

    let pattern = match name_pattern {
        Some(p) => p,
        None => bail!("Provide at least one of --name or --description."),
    };

    // Name search: fan out per target.
    eprintln!("{}", "Fetching metric names...".dimmed());
    let pattern = pattern.to_string();
    let per_profile = fan_out(targets, |t| {
        let p = pattern.clone();
        async move { execute_metric_names(&t.client, &p).await }
    })
    .await;

    let mut all_matches: Vec<(String, String)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(names) => {
                for n in names {
                    all_matches.push((profile.clone(), n));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json => {
            let names: Vec<&str> = all_matches.iter().map(|(_, n)| n.as_str()).collect();
            println!("{}", serde_json::to_string_pretty(&names)?);
        }
        OutputFormat::Agents => {
            if all_matches.is_empty() {
                println!("[]");
                return Ok(());
            }
            let rows: Vec<Value> = if include_profile {
                all_matches
                    .iter()
                    .map(|(profile, name)| json!({"profile": profile, "name": name}))
                    .collect()
            } else {
                all_matches
                    .iter()
                    .map(|(_, name)| json!({"name": name}))
                    .collect()
            };
            let toon =
                toon_encode(&rows).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_matches.is_empty() {
                println!("{}", "No metrics matched.".yellow());
                return Ok(());
            }
            if include_profile {
                let rows: Vec<MetricNameRow> = all_matches
                    .iter()
                    .map(|(profile, name)| MetricNameRow {
                        profile: profile.clone(),
                        metric_name: name.clone(),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            } else {
                let rows: Vec<MetricNameRowSingle> = all_matches
                    .iter()
                    .map(|(_, name)| MetricNameRowSingle {
                        metric_name: name.clone(),
                    })
                    .collect();
                println!("{}", Table::new(rows));
            }
        }
    }

    Ok(())
}

pub async fn run_get_labels(
    targets: &[Arc<ExecutionTarget>],
    metric: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching labels...".dimmed());

    let include_profile = targets.len() > 1;
    let metric = metric.to_string();
    let per_profile = fan_out(targets, |t| {
        let m = metric.clone();
        async move { execute_labels(&t.client, &m).await }
    })
    .await;

    let mut all_labels: Vec<(String, String)> = Vec::new();
    for (profile, result) in per_profile {
        match result {
            Ok(labels) => {
                for l in labels {
                    all_labels.push((profile.clone(), l));
                }
            }
            Err(e) => eprintln!("{}", format!("error from profile '{profile}': {e:#}").red()),
        }
    }

    match output {
        OutputFormat::Json | OutputFormat::Agents => {
            let json_rows: Vec<Value> = if include_profile {
                all_labels
                    .iter()
                    .map(|(profile, label)| json!({"profile": profile, "label": label}))
                    .collect()
            } else {
                all_labels
                    .iter()
                    .map(|(_, label)| json!({"label": label}))
                    .collect()
            };
            println!("{}", serde_json::to_string_pretty(&json_rows)?);
        }
        OutputFormat::Text => {
            if all_labels.is_empty() {
                println!("{}", "No labels found.".yellow());
                return Ok(());
            }
            let rows: Vec<LabelRow> = all_labels
                .iter()
                .map(|(_, l)| LabelRow { label: l.clone() })
                .collect();
            println!("{}", Table::new(rows));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
