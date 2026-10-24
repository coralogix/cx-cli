use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use colored::Colorize;
use serde_json::{json, Value};

pub mod api;

use api::{
    parse_category, parse_profile_type, profile_type_unit, FilterKeysResponse, FiltersResponse,
    FlameGraphNode, FlameGraphResponse, MetricsResponse, ProfilingApi, ProfilingFilter,
    ServiceOverview, ServiceQuery, ServicesParams, StringTable, TimeSample,
};

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;

const MAX_SERVICES_LIMIT: u32 = 100;
const SERVER_FLAME_GRAPH_PRUNE_RATIO: f64 = 0.001;

pub struct FlameGraphOptions {
    pub show_kernel: bool,
    pub min_node_ratio: f64,
    pub max_depth: Option<u32>,
    pub top_functions: usize,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_services(
    targets: &[Arc<ExecutionTarget>],
    category: &str,
    start: &str,
    end: &str,
    environments: &[String],
    limit: u32,
    offset: u32,
    include_time_series: bool,
    output: OutputFormat,
) -> Result<()> {
    let category_enum = parse_category(category)?;
    let category_key = category.trim().to_lowercase();
    let (start_epoch, end_epoch) = parse_time_range(start, end)?;
    if limit == 0 || limit > MAX_SERVICES_LIMIT {
        bail!("--limit must be between 1 and {MAX_SERVICES_LIMIT}; page with --offset to see more");
    }
    let environments: Vec<String> = environments
        .iter()
        .map(|e| validate_text(e, "--environment"))
        .collect::<Result<_>>()?;

    eprintln!(
        "{}",
        format!("Fetching profiled services by {category_key}...").dimmed()
    );

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| {
        let environments = environments.clone();
        let category_key = category_key.clone();
        async move {
            let api = ProfilingApi::new(&target.client);
            let params = ServicesParams {
                category: category_enum,
                start: start_epoch,
                end: end_epoch,
                environments: &environments,
                limit,
                offset,
            };
            let response = api.services(&params).await?;
            response
                .services
                .iter()
                .map(|s| format_service(s, &category_key, include_time_series))
                .collect::<Result<Vec<Value>>>()
        }
    })
    .await;

    let mut rows: Vec<Value> = Vec::new();
    for (profile, services) in report_errors_and_collect_successes(per_profile)? {
        if services.len() == limit as usize {
            eprintln!(
                "{}",
                format!(
                    "More services may exist{}; rerun with --offset {}.",
                    if include_profile {
                        format!(" in profile '{profile}'")
                    } else {
                        String::new()
                    },
                    offset + limit
                )
                .dimmed()
            );
        }
        for service in services {
            rows.push(tag_profile(service, include_profile, &profile));
        }
    }

    match output {
        OutputFormat::Json => render::render_json(&rows),
        OutputFormat::Toon => render::render_toon(&rows),
        OutputFormat::Text => {
            if rows.is_empty() {
                render::print_no_results("No profiled services found.");
                return Ok(());
            }
            let (headers, keys): (&[&str], [&str; 2]) = if category_key == "cpu" {
                (
                    &[
                        "Service",
                        "Language",
                        "Last Active",
                        "CPU Seconds",
                        "Wall Seconds",
                    ],
                    ["total_cpu_seconds", "total_wall_seconds"],
                )
            } else {
                (
                    &[
                        "Service",
                        "Language",
                        "Last Active",
                        "Allocated Bytes",
                        "Heap Bytes",
                    ],
                    ["total_memory_allocations_bytes", "total_heap_bytes"],
                )
            };
            let table_rows: Vec<Vec<String>> = rows
                .iter()
                .map(|row| {
                    let last_active = row["last_active"].as_i64().unwrap_or_default();
                    vec![
                        row["profile"].as_str().unwrap_or_default().to_string(),
                        row["service_name"].as_str().unwrap_or("-").to_string(),
                        row["language"].as_str().unwrap_or("-").to_string(),
                        chrono::DateTime::from_timestamp(last_active, 0)
                            .map(|d| d.to_rfc3339())
                            .unwrap_or_else(|| last_active.to_string()),
                        format_number(&row[keys[0]]),
                        format_number(&row[keys[1]]),
                    ]
                })
                .collect();
            render::render_table(headers, table_rows, include_profile);
            Ok(())
        }
    }
}

pub async fn run_metadata(
    targets: &[Arc<ExecutionTarget>],
    service_name: &str,
    start: &str,
    end: &str,
    output: OutputFormat,
) -> Result<()> {
    let service_name = validate_text(service_name, "service name")?;
    let (start_epoch, end_epoch) = parse_time_range(start, end)?;

    eprintln!(
        "{}",
        format!("Fetching profiling metadata for '{service_name}'...").dimmed()
    );

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| {
        let service_name = service_name.clone();
        async move {
            let api = ProfilingApi::new(&target.client);
            let response = api
                .service_metadata(&service_name, start_epoch, end_epoch)
                .await?;
            let service = response
                .service
                .filter(|s| !s.profile_types.is_empty())
                .ok_or_else(|| {
                    anyhow!(
                        "malformed profiling service metadata response: missing service profile types"
                    )
                })?;
            Ok(json!({
                "service_name": service_name,
                "language": service.language,
                "profile_types": service.profile_types.iter().map(|p| json!({
                    "profile_type": p.profile_type,
                    "has_data": p.has_data,
                })).collect::<Vec<_>>(),
                "profile_types_with_data": service.profile_types.iter()
                    .filter(|p| p.has_data)
                    .map(|p| p.profile_type.clone())
                    .collect::<Vec<_>>(),
            }))
        }
    })
    .await;

    let results = tag_results(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
    );
    if output != OutputFormat::Text {
        return render_machine_output(output, &results);
    }
    for val in &results {
        if include_profile {
            println!(
                "{}",
                format!("[{}]", val["profile"].as_str().unwrap_or_default()).dimmed()
            );
        }
        println!(
            "{}: {}",
            "Language".bold(),
            val["language"].as_str().unwrap_or("-")
        );
        let rows: Vec<Vec<String>> = val["profile_types"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| {
                vec![
                    String::new(),
                    p["profile_type"].as_str().unwrap_or("-").to_string(),
                    render::bool_display(p["has_data"].as_bool()),
                ]
            })
            .collect();
        render::render_table(&["Profile Type", "Has Data"], rows, false);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_filters(
    targets: &[Arc<ExecutionTarget>],
    service_name: &str,
    profile_type: &str,
    start: &str,
    end: &str,
    filters: &[String],
    max_values_per_key: usize,
    output: OutputFormat,
) -> Result<()> {
    let service_name = validate_text(service_name, "service name")?;
    let profile_type = parse_profile_type(profile_type)?;
    let (start_epoch, end_epoch) = parse_time_range(start, end)?;
    let filters = parse_filters(filters)?;
    if max_values_per_key == 0 {
        bail!("--max-values-per-key must be > 0");
    }

    eprintln!(
        "{}",
        format!("Fetching profiling filters for '{service_name}'...").dimmed()
    );

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| {
        let service_name = service_name.clone();
        let filters = filters.clone();
        async move {
            let api = ProfilingApi::new(&target.client);
            let query = ServiceQuery {
                service_name: &service_name,
                profile_type,
                start: start_epoch,
                end: end_epoch,
                filters: &filters,
            };
            let (keys, values) = futures::try_join!(api.filter_keys(&query), api.filters(&query))?;
            Ok(json!({
                "service_name": service_name,
                "profile_type": profile_type,
                "filters": format_filters(&keys, &values, max_values_per_key),
            }))
        }
    })
    .await;

    let results = tag_results(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
    );
    if output != OutputFormat::Text {
        return render_machine_output(output, &results);
    }
    for val in &results {
        if include_profile {
            println!(
                "{}",
                format!("[{}]", val["profile"].as_str().unwrap_or_default()).dimmed()
            );
        }
        let rows: Vec<Vec<String>> = val["filters"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                let values: Vec<&str> = f["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str())
                    .collect();
                vec![
                    String::new(),
                    f["key"].as_str().unwrap_or("-").to_string(),
                    f["total_values"].to_string(),
                    values.join(", "),
                ]
            })
            .collect();
        render::render_table(&["Key", "Total Values", "Values"], rows, false);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_metrics(
    targets: &[Arc<ExecutionTarget>],
    service_name: &str,
    profile_type: &str,
    start: &str,
    end: &str,
    filters: &[String],
    sum_by: &str,
    max_buckets: u32,
    limit: usize,
    output: OutputFormat,
) -> Result<()> {
    let service_name = validate_text(service_name, "service name")?;
    let profile_type = parse_profile_type(profile_type)?;
    let (start_epoch, end_epoch) = parse_time_range(start, end)?;
    let filters = parse_filters(filters)?;
    let sum_by = validate_text(sum_by, "--sum-by")?;
    if max_buckets == 0 {
        bail!("--max-buckets must be > 0");
    }
    if limit == 0 {
        bail!("--limit must be > 0");
    }

    eprintln!(
        "{}",
        format!("Fetching profiling metrics for '{service_name}'...").dimmed()
    );

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| {
        let service_name = service_name.clone();
        let filters = filters.clone();
        let sum_by = sum_by.clone();
        async move {
            let api = ProfilingApi::new(&target.client);
            let query = ServiceQuery {
                service_name: &service_name,
                profile_type,
                start: start_epoch,
                end: end_epoch,
                filters: &filters,
            };
            let response = api.metrics(&query, &sum_by, max_buckets).await?;
            let (total_series, series) = format_metrics(&response, limit);
            Ok(json!({
                "service_name": service_name,
                "profile_type": profile_type,
                "unit": profile_type_unit(profile_type),
                "sum_by": sum_by,
                "total_series": total_series,
                "series": series,
            }))
        }
    })
    .await;

    let results = tag_results(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
    );
    if output != OutputFormat::Text {
        return render_machine_output(output, &results);
    }
    for val in &results {
        if include_profile {
            println!(
                "{}",
                format!("[{}]", val["profile"].as_str().unwrap_or_default()).dimmed()
            );
        }
        println!(
            "{}: {} ({} series, unit: {})",
            "Sum By".bold(),
            val["sum_by"].as_str().unwrap_or("-"),
            val["total_series"],
            val["unit"].as_str().unwrap_or("-")
        );
        let rows: Vec<Vec<String>> = val["series"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                let peak = s["points"].as_array().and_then(|points| {
                    points.iter().max_by(|a, b| {
                        let a = a["total_value"].as_f64().unwrap_or_default();
                        let b = b["total_value"].as_f64().unwrap_or_default();
                        a.total_cmp(&b)
                    })
                });
                let peak_at = peak
                    .and_then(|p| p["timestamp"].as_i64())
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_else(|| "-".to_string());
                vec![
                    String::new(),
                    s["group_by_value"].as_str().unwrap_or("-").to_string(),
                    format_number(&s["total"]),
                    peak.map(|p| format_number(&p["total_value"]))
                        .unwrap_or_else(|| "-".to_string()),
                    peak_at,
                ]
            })
            .collect();
        render::render_table(&["Group", "Total", "Peak Bucket", "Peak At"], rows, false);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_flame_graph(
    targets: &[Arc<ExecutionTarget>],
    service_name: &str,
    profile_type: &str,
    start: &str,
    end: &str,
    filters: &[String],
    options: FlameGraphOptions,
    output: OutputFormat,
) -> Result<()> {
    let service_name = validate_text(service_name, "service name")?;
    let profile_type = parse_profile_type(profile_type)?;
    let (start_epoch, end_epoch) = parse_time_range(start, end)?;
    let filters = parse_filters(filters)?;
    if !(SERVER_FLAME_GRAPH_PRUNE_RATIO..1.0).contains(&options.min_node_ratio) {
        bail!(
            "--min-node-ratio must be >= {SERVER_FLAME_GRAPH_PRUNE_RATIO} (the server's own \
             prune ratio) and < 1"
        );
    }
    if options.max_depth == Some(0) {
        bail!("--max-depth must be > 0");
    }
    if options.top_functions == 0 {
        bail!("--top-functions must be > 0");
    }

    eprintln!(
        "{}",
        format!("Fetching flame graph for '{service_name}' (this can take a while)...").dimmed()
    );

    let include_profile = targets.len() > 1;
    let options = &options;

    let per_profile = fan_out(targets, |target| {
        let service_name = service_name.clone();
        let filters = filters.clone();
        async move {
            let api = ProfilingApi::new(&target.client);
            let query = ServiceQuery {
                service_name: &service_name,
                profile_type,
                start: start_epoch,
                end: end_epoch,
                filters: &filters,
            };
            let response = api.flame_graph(&query, options.show_kernel).await?;
            let mut formatted = format_flame_graph(response, options)?;
            formatted["service_name"] = json!(service_name);
            formatted["profile_type"] = json!(profile_type);
            Ok(formatted)
        }
    })
    .await;

    let results = tag_results(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
    );
    if output != OutputFormat::Text {
        return render_machine_output(output, &results);
    }
    for val in &results {
        if include_profile {
            println!(
                "{}",
                format!("[{}]", val["profile"].as_str().unwrap_or_default()).dimmed()
            );
        }
        println!("{}: {}", "Total".bold(), format_number(&val["total_value"]));
        for warning in val["warnings"].as_array().into_iter().flatten() {
            println!(
                "{}",
                format!(
                    "Warning: query hit its {} limit ({})",
                    warning["type"].as_str().unwrap_or("-"),
                    warning["limit"]
                )
                .yellow()
            );
        }
        println!();
        println!("{}", "Top functions by self value".bold());
        let rows: Vec<Vec<String>> = val["top_functions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                vec![
                    String::new(),
                    f["function"].as_str().unwrap_or("-").to_string(),
                    f["file"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("-")
                        .to_string(),
                    format_number(&f["self"]),
                    format!("{}%", f["self_percent"]),
                ]
            })
            .collect();
        render::render_table(&["Function", "File", "Self", "Self %"], rows, false);
        println!();
        println!(
            "{} ({} subtrees below --min-node-ratio/--max-depth omitted)",
            "Call tree (total %, self %)".bold(),
            val["omitted_subtrees"]
        );
        print_tree(&val["tree"], 0);
    }
    Ok(())
}

fn parse_time_range(start: &str, end: &str) -> Result<(i64, i64)> {
    let start_epoch = crate::time::parse_timestamp_epoch_seconds(start)?;
    let end_epoch = crate::time::parse_timestamp_epoch_seconds(end)?;
    if end_epoch <= start_epoch {
        bail!("--end ({end}) must be strictly after --start ({start})");
    }
    Ok((start_epoch, end_epoch))
}

fn validate_text(value: &str, name: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{name} must not be empty");
    }
    if trimmed.contains('\'') || trimmed.contains('\\') {
        bail!("{name} must not contain quotes or backslashes");
    }
    Ok(trimmed.to_string())
}

fn parse_filters(filters: &[String]) -> Result<Vec<ProfilingFilter>> {
    filters
        .iter()
        .map(|raw| {
            let (key, predicate, values) = match raw.split_once('=') {
                Some((key, values)) => match key.strip_suffix('!') {
                    Some(key) => (key, "NOT_EQUAL", Some(values)),
                    None => (key, "EQUAL", Some(values)),
                },
                None => match raw.rsplit_once(':') {
                    Some((key, "exists")) => (key, "EXISTS", None),
                    Some((key, "not-exists")) => (key, "NOT_EXISTS", None),
                    _ => bail!(
                        "invalid --filter '{raw}': expected key=v1,v2, key!=v1,v2, key:exists \
                         or key:not-exists"
                    ),
                },
            };
            let key = validate_text(key, &format!("--filter '{raw}' key"))?;
            let values: Vec<String> = values
                .map(|v| {
                    v.split(',')
                        .map(|value| validate_text(value, &format!("--filter '{raw}' value")))
                        .collect::<Result<_>>()
                })
                .transpose()?
                .unwrap_or_default();
            Ok(ProfilingFilter {
                key,
                values,
                predicate,
            })
        })
        .collect()
}

fn format_service(
    service: &ServiceOverview,
    category: &str,
    include_time_series: bool,
) -> Result<Value> {
    if service.service_name.is_empty() {
        bail!("malformed profiling services response: a service has no service_name");
    }
    let mut formatted = json!({
        "service_name": service.service_name,
        "language": service.language,
        "last_active": service.last_active,
    });
    match (category, &service.cpu, &service.memory) {
        ("cpu", Some(cpu), None) => {
            formatted["total_cpu_seconds"] = json!(cpu.total_cpu_seconds);
            formatted["cpu_samples_per_second"] = json!(cpu.samples_per_sec);
            formatted["total_wall_seconds"] = json!(cpu.total_wall_seconds);
            formatted["wall_samples_per_second"] = json!(cpu.wall_samples_per_second);
            if include_time_series {
                formatted["cpu_series"] = time_samples_to_json(&cpu.series);
                formatted["wall_series"] = time_samples_to_json(&cpu.wall_series);
            }
        }
        ("memory", None, Some(memory)) => {
            formatted["total_memory_allocations_bytes"] =
                json!(memory.total_memory_allocations_bytes);
            formatted["total_memory_allocations_count"] =
                json!(memory.total_memory_allocations_count);
            formatted["total_heap_bytes"] = json!(memory.total_heap_bytes);
            formatted["total_heap_objects"] = json!(memory.total_heap_objects);
            if include_time_series {
                formatted["allocations_series"] = time_samples_to_json(&memory.series);
                formatted["heap_bytes_series"] = time_samples_to_json(&memory.heap_bytes_series);
            }
        }
        _ => bail!(
            "malformed profiling services response: service '{}' does not carry exactly {} data",
            service.service_name,
            category
        ),
    }
    Ok(formatted)
}

fn format_filters(
    keys: &FilterKeysResponse,
    values: &FiltersResponse,
    max_values_per_key: usize,
) -> Vec<Value> {
    let all_keys: BTreeSet<&String> = keys
        .filter_keys
        .iter()
        .chain(values.filters.keys())
        .collect();
    all_keys
        .into_iter()
        .map(|key| {
            let mut key_values: Vec<&String> = values
                .filters
                .get(key)
                .map(|v| v.values.iter().collect())
                .unwrap_or_default();
            key_values.sort();
            json!({
                "key": key,
                "values": key_values.iter().take(max_values_per_key).collect::<Vec<_>>(),
                "total_values": key_values.len(),
                "truncated": key_values.len() > max_values_per_key,
            })
        })
        .collect()
}

fn format_metrics(response: &MetricsResponse, limit: usize) -> (usize, Vec<Value>) {
    let mut series: Vec<(f64, Value)> = response
        .series
        .iter()
        .map(|s| {
            let total: f64 = s.points.iter().map(|p| p.total_value).sum();
            let points: Vec<Value> = s
                .points
                .iter()
                .map(|p| {
                    json!({
                        "timestamp": p.timestamp,
                        "avg_value": p.avg_value,
                        "total_value": p.total_value,
                    })
                })
                .collect();
            (
                total,
                json!({ "group_by_value": s.group_by_value, "total": total, "points": points }),
            )
        })
        .collect();
    series.sort_by(|a, b| b.0.total_cmp(&a.0));
    let total_series = series.len();
    (
        total_series,
        series.into_iter().take(limit).map(|(_, s)| s).collect(),
    )
}

fn time_samples_to_json(samples: &[TimeSample]) -> Value {
    samples
        .iter()
        .map(|s| json!({ "timestamp": s.timestamp, "value": s.value }))
        .collect()
}

fn decode_string_table(table: &StringTable) -> Result<Vec<String>> {
    if table.offset.len() != table.length.len() {
        bail!("malformed profiling flame graph: string table offset and length counts differ");
    }
    let bytes = STANDARD
        .decode(&table.strings)
        .map_err(|e| anyhow!("malformed profiling flame graph: string table is not base64: {e}"))?;
    table
        .offset
        .iter()
        .zip(&table.length)
        .map(|(&offset, &length)| {
            let entry = bytes.get(offset..offset + length).ok_or_else(|| {
                anyhow!(
                    "malformed profiling flame graph: string table entry exceeds the strings blob"
                )
            })?;
            String::from_utf8(entry.to_vec()).map_err(|_| {
                anyhow!("malformed profiling flame graph: string table entry is not UTF-8")
            })
        })
        .collect()
}

fn lookup_string(strings: &[String], index: usize) -> Result<&str> {
    strings.get(index).map(String::as_str).ok_or_else(|| {
        anyhow!(
            "malformed profiling flame graph: string index {index} is out of range for a table \
             of {}",
            strings.len()
        )
    })
}

fn resolve_frame(node: &FlameGraphNode, strings: &[String]) -> Result<Value> {
    let mut frame = json!({ "frame_type": node.frame_type });
    let Some(location) = node.function_locations.first() else {
        frame["function"] = json!(if node.address.is_empty() {
            "unknown"
        } else {
            node.address.as_str()
        });
        return Ok(frame);
    };
    let name = match location.function_name_index {
        Some(index) => lookup_string(strings, index)?,
        None => "",
    };
    let system_name = match location.function_system_name_index {
        Some(index) => lookup_string(strings, index)?,
        None => "",
    };
    let file = lookup_string(strings, location.function_filename_index)?;
    let function = [name, system_name, file, node.address.as_str()]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or("unknown");
    frame["function"] = json!(function);
    if !file.is_empty() {
        frame["file"] = json!(file);
    }
    if let Some(line) = &location.line {
        frame["line"] = line.clone();
    }
    if !system_name.is_empty() && system_name != function {
        frame["system_name"] = json!(system_name);
    }
    if let Some(index) = location.package_name_index {
        frame["package"] = json!(lookup_string(strings, index)?);
    }
    let classification = match (location.classification.as_str(), node.frame_type.as_str()) {
        ("FRAME_CLASSIFICATION_UNSPECIFIED" | "", "FRAME_TYPE_NATIVE") => {
            "FRAME_CLASSIFICATION_NATIVE"
        }
        ("FRAME_CLASSIFICATION_UNSPECIFIED" | "", "FRAME_TYPE_KERNEL") => {
            "FRAME_CLASSIFICATION_KERNEL"
        }
        (classification, _) => classification,
    };
    frame["classification"] = json!(classification);
    Ok(frame)
}

fn percent(value: f64, total: f64) -> f64 {
    if total == 0.0 {
        return 0.0;
    }
    (value / total * 10_000.0).round() / 100.0
}

fn sum_self_values(
    node: &FlameGraphNode,
    strings: &[String],
    self_values: &mut HashMap<(String, String), f64>,
) -> Result<()> {
    let frame = resolve_frame(node, strings)?;
    let key = (
        frame["function"].as_str().unwrap_or_default().to_string(),
        frame["file"].as_str().unwrap_or_default().to_string(),
    );
    *self_values.entry(key).or_default() += node.self_value;
    for child in &node.children {
        sum_self_values(child, strings, self_values)?;
    }
    Ok(())
}

fn build_tree(
    node: &FlameGraphNode,
    strings: &[String],
    depth: u32,
    min_node_value: f64,
    max_depth: Option<u32>,
    root_total: f64,
    omitted_subtrees: &mut usize,
) -> Result<Value> {
    let mut frame = resolve_frame(node, strings)?;
    let mut children: Vec<&FlameGraphNode> = node.children.iter().collect();
    children.sort_by(|a, b| b.total_value.total_cmp(&a.total_value));
    let mut kept = Vec::new();
    for child in children {
        let within_depth = max_depth.is_none_or(|max| depth < max);
        if within_depth && child.total_value >= min_node_value {
            kept.push(build_tree(
                child,
                strings,
                depth + 1,
                min_node_value,
                max_depth,
                root_total,
                omitted_subtrees,
            )?);
        } else {
            *omitted_subtrees += 1;
        }
    }
    frame["self"] = json!(node.self_value);
    frame["total"] = json!(node.total_value);
    frame["self_percent"] = json!(percent(node.self_value, root_total));
    frame["total_percent"] = json!(percent(node.total_value, root_total));
    frame["children"] = Value::Array(kept);
    Ok(frame)
}

fn format_flame_graph(response: FlameGraphResponse, options: &FlameGraphOptions) -> Result<Value> {
    let (Some(root), Some(string_table)) = (&response.flamegraph, &response.string_table) else {
        bail!("malformed profiling flame graph response: missing flamegraph or stringTable");
    };
    let strings = decode_string_table(string_table)?;
    let total = root.total_value;

    let mut self_values: HashMap<(String, String), f64> = HashMap::new();
    sum_self_values(root, &strings, &mut self_values)?;
    let mut ranked: Vec<((String, String), f64)> = self_values
        .into_iter()
        .filter(|(_, value)| *value > 0.0)
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_functions: Vec<Value> = ranked
        .into_iter()
        .take(options.top_functions)
        .map(|((function, file), self_value)| {
            json!({
                "function": function,
                "file": file,
                "self": self_value,
                "self_percent": percent(self_value, total),
            })
        })
        .collect();

    let mut omitted_subtrees = 0;
    let tree = build_tree(
        root,
        &strings,
        0,
        total * options.min_node_ratio,
        options.max_depth,
        total,
        &mut omitted_subtrees,
    )?;

    let warnings: Vec<Value> = response
        .warn_messages
        .iter()
        .map(|warning| match warning.as_object() {
            Some(obj) if obj.len() == 1 => {
                let (warning_type, detail) = obj.iter().next().expect("one entry");
                Ok(json!({ "type": warning_type, "limit": detail.get("value") }))
            }
            _ => Err(anyhow!(
                "malformed profiling flame graph: unexpected query warning {warning}"
            )),
        })
        .collect::<Result<_>>()?;

    Ok(json!({
        "was_seen": response.was_seen,
        "warnings": warnings,
        "total_value": total,
        "server_prune_ratio": SERVER_FLAME_GRAPH_PRUNE_RATIO,
        "min_node_ratio": options.min_node_ratio,
        "max_depth": options.max_depth,
        "omitted_subtrees": omitted_subtrees,
        "top_functions": top_functions,
        "tree": tree,
    }))
}

fn print_tree(node: &Value, depth: usize) {
    let location = match (node["file"].as_str(), node.get("line")) {
        (Some(file), Some(line)) => format!(" ({file}:{line})"),
        (Some(file), None) => format!(" ({file})"),
        _ => String::new(),
    };
    println!(
        "{}{:>6.2}% {:>6.2}%  {}{}",
        "  ".repeat(depth),
        node["total_percent"].as_f64().unwrap_or_default(),
        node["self_percent"].as_f64().unwrap_or_default(),
        node["function"].as_str().unwrap_or("unknown"),
        location.dimmed()
    );
    for child in node["children"].as_array().into_iter().flatten() {
        print_tree(child, depth + 1);
    }
}

fn format_number(value: &Value) -> String {
    match value.as_f64() {
        Some(n) if n.fract() == 0.0 => format!("{n:.0}"),
        Some(n) => format!("{n:.2}"),
        None => "-".to_string(),
    }
}

fn tag_profile(mut value: Value, include_profile: bool, profile: &str) -> Value {
    if include_profile {
        value["profile"] = json!(profile);
    }
    value
}

fn tag_results(results: Vec<(String, Value)>, include_profile: bool) -> Vec<Value> {
    results
        .into_iter()
        .map(|(profile, value)| tag_profile(value, include_profile, &profile))
        .collect()
}

fn render_machine_output(output: OutputFormat, results: &[Value]) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json_auto(results),
        OutputFormat::Toon => render::render_toon(results),
        OutputFormat::Text => {
            unreachable!("callers render text themselves; only Json/Toon reach here")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> FlameGraphOptions {
        FlameGraphOptions {
            show_kernel: false,
            min_node_ratio: 0.01,
            max_depth: None,
            top_functions: 20,
        }
    }

    fn string_table(strings: &[&str]) -> StringTable {
        let mut blob = String::new();
        let mut offset = Vec::new();
        let mut length = Vec::new();
        for s in strings {
            offset.push(blob.len());
            length.push(s.len());
            blob.push_str(s);
        }
        StringTable {
            strings: STANDARD.encode(blob),
            offset,
            length,
        }
    }

    fn flame_graph_response() -> FlameGraphResponse {
        serde_json::from_value(json!({
            "flamegraph": {
                "functionLocations": [{ "functionFilenameIndex": 0, "functionNameIndex": 1 }],
                "frameType": "FRAME_TYPE_UNSPECIFIED",
                "totalValue": 1000.0,
                "selfValue": 0.0,
                "children": [
                    {
                        "functionLocations": [{ "functionFilenameIndex": 2, "functionNameIndex": 3, "line": 7 }],
                        "frameType": "FRAME_TYPE_GO",
                        "totalValue": 900.0,
                        "selfValue": 400.0,
                        "children": [{
                            "functionLocations": [{ "functionFilenameIndex": 2, "functionNameIndex": 4 }],
                            "frameType": "FRAME_TYPE_GO",
                            "totalValue": 500.0,
                            "selfValue": 500.0,
                            "children": []
                        }]
                    },
                    {
                        "address": "0xdead",
                        "functionLocations": [],
                        "frameType": "FRAME_TYPE_NATIVE",
                        "totalValue": 95.0,
                        "selfValue": 95.0,
                        "children": []
                    },
                    {
                        "functionLocations": [{ "functionFilenameIndex": 2, "functionNameIndex": 4 }],
                        "frameType": "FRAME_TYPE_GO",
                        "totalValue": 5.0,
                        "selfValue": 5.0,
                        "children": []
                    }
                ]
            },
            "stringTable": {
                "strings": STANDARD.encode("rootmain.gohandleparse"),
                "offset": [0, 0, 4, 11, 17],
                "length": [0, 4, 7, 6, 5]
            },
            "warnMessages": [{ "maxBytes": { "value": "100" } }],
            "wasSeen": true
        }))
        .unwrap()
    }

    #[test]
    fn parse_filters_supports_every_predicate() {
        let filters = parse_filters(&[
            "k8s.pod.name=a,b".to_string(),
            "service.version!=1.0".to_string(),
            "host.name:exists".to_string(),
            "k8s.namespace.name:not-exists".to_string(),
        ])
        .unwrap();
        assert_eq!(filters[0].predicate, "EQUAL");
        assert_eq!(filters[0].values, vec!["a", "b"]);
        assert_eq!(filters[1].key, "service.version");
        assert_eq!(filters[1].predicate, "NOT_EQUAL");
        assert_eq!(filters[2].predicate, "EXISTS");
        assert!(filters[2].values.is_empty());
        assert_eq!(filters[3].key, "k8s.namespace.name");
        assert_eq!(filters[3].predicate, "NOT_EXISTS");
    }

    #[test]
    fn parse_filters_rejects_bad_input() {
        assert!(parse_filters(&["k8s.pod.name".to_string()]).is_err());
        assert!(parse_filters(&["=a".to_string()]).is_err());
        assert!(parse_filters(&["key=a,,b".to_string()]).is_err());
        assert!(parse_filters(&["key=it's".to_string()]).is_err());
        assert!(parse_filters(&["key:maybe".to_string()]).is_err());
    }

    #[test]
    fn validate_text_trims_and_rejects_quotes() {
        assert_eq!(validate_text(" checkout ", "name").unwrap(), "checkout");
        assert!(validate_text("  ", "name").is_err());
        assert!(validate_text("a\\b", "name").is_err());
    }

    #[test]
    fn parse_time_range_requires_end_after_start() {
        assert!(parse_time_range("now-1h", "now").is_ok());
        assert!(parse_time_range("now", "now-1h").is_err());
    }

    #[test]
    fn format_service_requires_matching_category_data() {
        let service: ServiceOverview = serde_json::from_value(json!({
            "serviceName": "checkout",
            "lastActive": "5",
            "cpu": { "totalCpuSeconds": 12.5, "series": [{ "timestamp": "1", "value": 2.0 }] }
        }))
        .unwrap();
        let formatted = format_service(&service, "cpu", false).unwrap();
        assert_eq!(formatted["total_cpu_seconds"], json!(12.5));
        assert!(formatted.get("cpu_series").is_none());
        let with_series = format_service(&service, "cpu", true).unwrap();
        assert_eq!(with_series["cpu_series"][0]["value"], json!(2.0));
        assert!(format_service(&service, "memory", false).is_err());
    }

    #[test]
    fn format_service_rejects_missing_name() {
        let service = ServiceOverview::default();
        assert!(format_service(&service, "cpu", false).is_err());
    }

    #[test]
    fn decode_string_table_slices_the_blob() {
        let strings = decode_string_table(&string_table(&["", "main", "handle"])).unwrap();
        assert_eq!(strings, vec!["", "main", "handle"]);
    }

    #[test]
    fn decode_string_table_rejects_out_of_range_entry() {
        let table = StringTable {
            strings: STANDARD.encode("abc"),
            offset: vec![2],
            length: vec![5],
        };
        assert!(decode_string_table(&table).is_err());
    }

    #[test]
    fn decode_string_table_rejects_mismatched_counts() {
        let table = StringTable {
            strings: STANDARD.encode("abc"),
            offset: vec![0, 1],
            length: vec![1],
        };
        assert!(decode_string_table(&table).is_err());
    }

    #[test]
    fn resolve_frame_falls_back_through_names() {
        let strings = vec!["".to_string(), "sys_name".to_string(), "file.c".to_string()];
        let node: FlameGraphNode = serde_json::from_value(json!({
            "functionLocations": [{ "functionFilenameIndex": 2, "functionSystemNameIndex": 1 }],
            "frameType": "FRAME_TYPE_NATIVE"
        }))
        .unwrap();
        let frame = resolve_frame(&node, &strings).unwrap();
        assert_eq!(frame["function"], "sys_name");
        assert_eq!(frame["file"], "file.c");
        assert!(frame.get("system_name").is_none());
        assert_eq!(frame["classification"], "FRAME_CLASSIFICATION_NATIVE");

        let file_only: FlameGraphNode = serde_json::from_value(json!({
            "functionLocations": [{ "functionFilenameIndex": 2 }]
        }))
        .unwrap();
        assert_eq!(
            resolve_frame(&file_only, &strings).unwrap()["function"],
            "file.c"
        );

        let address_only: FlameGraphNode =
            serde_json::from_value(json!({ "address": "0x1" })).unwrap();
        assert_eq!(
            resolve_frame(&address_only, &strings).unwrap()["function"],
            "0x1"
        );

        let empty = FlameGraphNode::default();
        assert_eq!(
            resolve_frame(&empty, &strings).unwrap()["function"],
            "unknown"
        );
    }

    #[test]
    fn resolve_frame_rejects_out_of_range_index() {
        let node: FlameGraphNode = serde_json::from_value(json!({
            "functionLocations": [{ "functionFilenameIndex": 9 }]
        }))
        .unwrap();
        assert!(resolve_frame(&node, &[]).is_err());
    }

    #[test]
    fn format_flame_graph_ranks_top_functions_across_the_whole_tree() {
        let formatted = format_flame_graph(flame_graph_response(), &options()).unwrap();
        let top = formatted["top_functions"].as_array().unwrap();
        assert_eq!(top[0]["function"], "parse");
        assert_eq!(top[0]["self"], json!(505.0));
        assert_eq!(top[0]["self_percent"], json!(50.5));
        assert_eq!(top[1]["function"], "handle");
        assert_eq!(top[1]["file"], "main.go");
        assert_eq!(top[2]["function"], "0xdead");
        assert_eq!(top.len(), 3);
        assert_eq!(
            formatted["warnings"],
            json!([{ "type": "maxBytes", "limit": "100" }])
        );
    }

    #[test]
    fn format_flame_graph_trims_small_nodes_and_sorts_children() {
        let formatted = format_flame_graph(flame_graph_response(), &options()).unwrap();
        let tree = &formatted["tree"];
        assert_eq!(tree["function"], "root");
        let children = tree["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["function"], "handle");
        assert_eq!(children[0]["line"], json!(7));
        assert_eq!(children[0]["total_percent"], json!(90.0));
        assert_eq!(children[1]["function"], "0xdead");
        assert_eq!(formatted["omitted_subtrees"], json!(1));
    }

    #[test]
    fn format_flame_graph_honors_max_depth() {
        let mut opts = options();
        opts.max_depth = Some(1);
        let formatted = format_flame_graph(flame_graph_response(), &opts).unwrap();
        let handle = &formatted["tree"]["children"][0];
        assert!(handle["children"].as_array().unwrap().is_empty());
        assert_eq!(formatted["omitted_subtrees"], json!(2));
        assert_eq!(formatted["top_functions"][0]["function"], "parse");
    }

    #[test]
    fn format_flame_graph_rejects_missing_parts() {
        let response = FlameGraphResponse::default();
        assert!(format_flame_graph(response, &options()).is_err());
    }

    #[test]
    fn format_filters_merges_keys_and_truncates_sorted_values() {
        let keys: FilterKeysResponse =
            serde_json::from_value(json!({ "filterKeys": ["k8s.pod.name", "host.name"] })).unwrap();
        let values: FiltersResponse = serde_json::from_value(json!({
            "filters": {
                "k8s.pod.name": { "values": ["pod-c", "pod-a", "pod-b"] },
                "service.version": { "values": ["1.0"] }
            }
        }))
        .unwrap();
        let formatted = format_filters(&keys, &values, 2);
        let keys: Vec<&str> = formatted
            .iter()
            .map(|f| f["key"].as_str().unwrap())
            .collect();
        assert_eq!(keys, vec!["host.name", "k8s.pod.name", "service.version"]);
        assert_eq!(formatted[0]["values"], json!([]));
        assert_eq!(formatted[1]["values"], json!(["pod-a", "pod-b"]));
        assert_eq!(formatted[1]["total_values"], json!(3));
        assert_eq!(formatted[1]["truncated"], json!(true));
        assert_eq!(formatted[2]["truncated"], json!(false));
    }

    #[test]
    fn format_metrics_ranks_by_summed_total_and_trims() {
        let response: MetricsResponse = serde_json::from_value(json!({
            "series": [
                { "groupByValue": "small", "points": [{ "timestamp": "1", "totalValue": 1.0 }] },
                { "groupByValue": "big", "points": [
                    { "timestamp": "1", "totalValue": 5.0 },
                    { "timestamp": "2", "totalValue": 6.0 }
                ] },
                { "groupByValue": "mid", "points": [{ "timestamp": "1", "totalValue": 10.0 }] }
            ]
        }))
        .unwrap();
        let (total_series, series) = format_metrics(&response, 2);
        assert_eq!(total_series, 3);
        assert_eq!(series.len(), 2);
        assert_eq!(series[0]["group_by_value"], "big");
        assert_eq!(series[0]["total"], json!(11.0));
        assert_eq!(series[0]["points"][1]["timestamp"], json!(2));
        assert_eq!(series[1]["group_by_value"], "mid");
    }

    #[test]
    fn percent_rounds_and_handles_zero_total() {
        assert_eq!(percent(1.0, 3.0), 33.33);
        assert_eq!(percent(1.0, 0.0), 0.0);
    }

    #[test]
    fn format_number_drops_trailing_zeros_for_integers() {
        assert_eq!(format_number(&json!(12.0)), "12");
        assert_eq!(format_number(&json!(1.234)), "1.23");
        assert_eq!(format_number(&json!("x")), "-");
    }
}
