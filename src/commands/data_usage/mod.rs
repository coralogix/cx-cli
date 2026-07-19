pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{collect_successes, fan_out, ExecutionTarget};
use crate::render;
use api::DataUsageApi;

// ── Subcommand runners ────────────────────────────────────────────────────────

pub struct CountCommandOptions<'a> {
    pub start: Option<&'a str>,
    pub end: Option<&'a str>,
    pub resolution: Option<&'a str>,
    pub subsystem_aggregation: bool,
    pub application_aggregation: bool,
    pub extra_params: &'a [String],
    pub output: OutputFormat,
}

fn build_count_query_params(options: &CountCommandOptions<'_>) -> Result<Vec<(String, String)>> {
    let from = match options
        .start
        .map(crate::time::parse_timestamp)
        .transpose()?
    {
        Some(s) => s,
        None => chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::hours(24))
            .unwrap_or_else(chrono::Utc::now)
            .to_rfc3339(),
    };
    let to = match options.end.map(crate::time::parse_timestamp).transpose()? {
        Some(s) => s,
        None => chrono::Utc::now().to_rfc3339(),
    };
    let mut params = vec![
        ("date_range.fromDate".to_string(), from),
        ("date_range.toDate".to_string(), to),
    ];

    params.push((
        "resolution".to_string(),
        options.resolution.unwrap_or("1h").to_string(),
    ));
    if options.subsystem_aggregation {
        params.push(("subsystem_aggregation".to_string(), "true".to_string()));
    }
    if options.application_aggregation {
        params.push(("application_aggregation".to_string(), "true".to_string()));
    }
    for raw in options.extra_params {
        let (key, value) = raw
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("query params must use KEY=VALUE format: {raw}"))?;
        if key.trim().is_empty() {
            anyhow::bail!("query param key cannot be empty: {raw}");
        }
        params.push((key.to_string(), value.to_string()));
    }

    Ok(params)
}

pub async fn run_summary(
    targets: &[Arc<ExecutionTarget>],
    start: Option<&str>,
    end: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching data usage summary...".dimmed());

    let include_profile = targets.len() > 1;
    let from = match start.map(crate::time::parse_timestamp).transpose()? {
        Some(s) => s,
        None => chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::hours(24))
            .unwrap_or_else(chrono::Utc::now)
            .to_rfc3339(),
    };
    let to = match end.map(crate::time::parse_timestamp).transpose()? {
        Some(s) => s,
        None => chrono::Utc::now().to_rfc3339(),
    };

    let per_profile = fan_out(targets, |t| {
        let from = from.clone();
        let to = to.clone();
        async move {
            let api = DataUsageApi::new(&t.client);
            Ok(api
                .get_usage(&[
                    ("date_range.fromDate", &from),
                    ("date_range.toDate", &to),
                    ("resolution", "1h"),
                ])
                .await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        all_results.push(val);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_results.is_empty() {
                render::print_no_results("No data usage found.");
                return Ok(());
            }
            for val in &all_results {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}

pub async fn run_daily(
    targets: &[Arc<ExecutionTarget>],
    data_type: &str,
    start: Option<&str>,
    end: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching daily {data_type} usage...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let data_type = data_type.to_string();
    let from = match start.map(crate::time::parse_timestamp).transpose()? {
        Some(s) => s,
        None => chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::hours(24))
            .unwrap_or_else(chrono::Utc::now)
            .to_rfc3339(),
    };
    let to = match end.map(crate::time::parse_timestamp).transpose()? {
        Some(s) => s,
        None => chrono::Utc::now().to_rfc3339(),
    };

    let per_profile = fan_out(targets, |t| {
        let data_type = data_type.clone();
        let from = from.clone();
        let to = to.clone();
        async move {
            let api = DataUsageApi::new(&t.client);
            let mut body = serde_json::Map::new();
            let mut date_range = serde_json::Map::new();
            date_range.insert("fromDate".into(), Value::String(from));
            date_range.insert("toDate".into(), Value::String(to));
            body.insert("date_range".into(), Value::Object(date_range));
            Ok(api.daily(&data_type, &Value::Object(body)).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        all_results.push(val);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_results.is_empty() {
                render::print_no_results("No daily usage data found.");
                return Ok(());
            }
            for val in &all_results {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}

pub async fn run_logs_count(
    targets: &[Arc<ExecutionTarget>],
    options: CountCommandOptions<'_>,
) -> Result<()> {
    eprintln!("{}", "Fetching logs count...".dimmed());

    let include_profile = targets.len() > 1;
    let params = build_count_query_params(&options)?;

    let per_profile = fan_out(targets, |t| {
        let params = params.clone();
        async move {
            let api = DataUsageApi::new(&t.client);
            let params_ref: Vec<(&str, &str)> = params
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            Ok(api.logs_count(&params_ref).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        all_results.push(val);
    }

    match options.output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            for val in &all_results {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}

pub async fn run_spans_count(
    targets: &[Arc<ExecutionTarget>],
    options: CountCommandOptions<'_>,
) -> Result<()> {
    eprintln!("{}", "Fetching spans count...".dimmed());

    let include_profile = targets.len() > 1;
    let params = build_count_query_params(&options)?;

    let per_profile = fan_out(targets, |t| {
        let params = params.clone();
        async move {
            let api = DataUsageApi::new(&t.client);
            let params_ref: Vec<(&str, &str)> = params
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            Ok(api.spans_count(&params_ref).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        all_results.push(val);
    }

    match options.output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            for val in &all_results {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}

pub async fn run_export_status(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching export status...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = DataUsageApi::new(&t.client);
        Ok(api.export_status().await?)
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        all_results.push(val);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            for val in &all_results {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }

    Ok(())
}
