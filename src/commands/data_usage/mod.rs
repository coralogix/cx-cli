pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
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

fn read_query_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading data usage query from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading data usage query from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };

    let query: Value = serde_json::from_str(&raw)?;
    if !query.is_object() {
        anyhow::bail!("data usage query must be a JSON object");
    }
    Ok(query)
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
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
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
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
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
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
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
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
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
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
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

/// Fetch the tenant's supported Data Usage Query API dimensions and limits.
pub async fn run_capabilities(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching data usage capabilities...".dimmed());

    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = DataUsageApi::new(&t.client);
        Ok(api.capabilities().await?)
    })
    .await;

    let mut all_results = Vec::new();
    for (profile, mut value) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut value, &profile);
        }
        all_results.push(value);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            for value in &all_results {
                println!("{}", serde_json::to_string_pretty(value)?);
            }
        }
    }

    Ok(())
}

/// Submit a capabilities-derived Data Usage Query API request.
pub async fn run_query(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let query = read_query_from_file(from_file)?;
    eprintln!("{}", "Querying data usage...".dimmed());

    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| {
        let query = query.clone();
        async move {
            let api = DataUsageApi::new(&t.client);
            Ok(api.query(&query).await?)
        }
    })
    .await;

    let mut all_results = Vec::new();
    for (profile, mut value) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut value, &profile);
        }
        all_results.push(value);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            for value in &all_results {
                println!("{}", serde_json::to_string_pretty(value)?);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::read_query_from_file;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_query(contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "cx-data-usage-query-unit-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock is after Unix epoch")
                .as_nanos()
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn read_query_from_file_accepts_json_object() {
        let path = write_query(r#"{"daily":{"relativeRange":"DAILY_RELATIVE_RANGE_LAST_7_DAYS"}}"#);
        let query = read_query_from_file(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(
            query["daily"]["relativeRange"],
            "DAILY_RELATIVE_RANGE_LAST_7_DAYS"
        );
    }

    #[test]
    fn read_query_from_file_rejects_non_object_json() {
        let path = write_query(r#"["not","a","query"]"#);
        let error = read_query_from_file(path.to_str().unwrap()).unwrap_err();
        std::fs::remove_file(path).unwrap();

        assert!(error.to_string().contains("must be a JSON object"));
    }
}
