use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

pub mod api;

use api::{
    normalize_entity_type, ApmFilterInput, ColumnMetadata, DataParams, EntitiesDataResult,
    EntityItem, EntityTypeInfo, EntityTypeMetadata, ServiceCatalogApi, TableRow,
};

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;

// ── Subcommand runners ────────────────────────────────────────────────────────

/// `cx service-catalog entity-types` - the entity types this account has data for.
pub async fn run_entity_types(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching entity types...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| async move {
        let api = ServiceCatalogApi::new(&target.client);
        Ok(api.list_entity_types().await?)
    })
    .await;

    let mut merged: Vec<(String, EntityTypeInfo)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for item in resp.entity_types {
            merged.push((profile.clone(), item));
        }
    }

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows: Vec<Value> = merged
                .iter()
                .map(|(profile, item)| entity_type_info_to_json(item, include_profile, profile))
                .collect();
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            if merged.is_empty() {
                render::print_no_results("No entity types found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = merged
                .iter()
                .map(|(profile, item)| {
                    vec![
                        profile.clone(),
                        display_or_dash(item.entity_type.as_deref()),
                        display_or_dash(item.id.as_deref()),
                        display_or_dash(item.display_name.as_deref()),
                    ]
                })
                .collect();
            render::render_table(
                &["Entity Type", "ID", "Display Name"],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}

/// `cx service-catalog schema <entity-type>` - columns/labels schema for one entity type.
pub async fn run_schema(
    targets: &[Arc<ExecutionTarget>],
    entity_type: &str,
    output: OutputFormat,
) -> Result<()> {
    let normalized = normalize_entity_type(entity_type)?;

    eprintln!(
        "{}",
        format!("Fetching schema for entity type '{entity_type}'...").dimmed()
    );

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| {
        let normalized = normalized.clone();
        async move {
            let api = ServiceCatalogApi::new(&target.client);
            Ok(api.entity_type_schema(&normalized).await?)
        }
    })
    .await;

    let mut results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        let mut val = entity_type_metadata_to_json(&resp);
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        results.push(val);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&results)?,
        OutputFormat::Toon => render::render_toon(&results)?,
        OutputFormat::Text => {
            render::render_get_text(
                &results,
                include_profile,
                "No schema found.",
                Some(&|val| {
                    println!(
                        "{}: {}",
                        "Display Name".bold(),
                        val.get("display_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                    );
                    println!(
                        "{}: {}",
                        "Description".bold(),
                        val.get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                    );
                    println!(
                        "{}: {}",
                        "Group By Limit".bold(),
                        val.get("group_by_limit")
                            .and_then(|v| v.as_i64())
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "-".to_string())
                    );
                }),
            )?;
        }
    }

    Ok(())
}

/// `cx service-catalog entities <entity-type>` - known entities of one entity type.
pub async fn run_entities(
    targets: &[Arc<ExecutionTarget>],
    entity_type: &str,
    output: OutputFormat,
) -> Result<()> {
    let normalized = normalize_entity_type(entity_type)?;

    eprintln!(
        "{}",
        format!("Fetching entities of type '{entity_type}'...").dimmed()
    );

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| {
        let normalized = normalized.clone();
        async move {
            let api = ServiceCatalogApi::new(&target.client);
            Ok(api.list_entities(&normalized).await?)
        }
    })
    .await;

    let mut merged: Vec<(String, EntityItem)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for item in resp.entities {
            merged.push((profile.clone(), item));
        }
    }

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows: Vec<Value> = merged
                .iter()
                .map(|(profile, item)| entity_item_to_json(item, include_profile, profile))
                .collect();
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            if merged.is_empty() {
                render::print_no_results("No entities found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = merged
                .iter()
                .map(|(profile, item)| {
                    vec![
                        profile.clone(),
                        display_or_dash(item.name.as_deref()),
                        display_or_dash(item.system.as_deref()),
                        display_or_dash(item.last_seen.as_deref()),
                    ]
                })
                .collect();
            render::render_table(&["Name", "System", "Last Seen"], rows, include_profile);
        }
    }

    Ok(())
}

/// `cx service-catalog data <entity-type>` - column data across every entity of one type.
#[allow(clippy::too_many_arguments)]
pub async fn run_data(
    targets: &[Arc<ExecutionTarget>],
    entity_type: &str,
    start: &str,
    end: &str,
    columns: &[String],
    group_by: &[String],
    filters: &[String],
    aggregation: &str,
    limit: Option<i32>,
    sort_column: Option<&str>,
    sort_order: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let normalized_entity_type = normalize_entity_type(entity_type)?;
    let (start_epoch, end_epoch) = parse_time_range(start, end)?;
    let validated_columns = validate_columns(columns)?;
    let validated_group_by = validate_group_by(group_by)?;
    let validated_filters = parse_filters(filters)?;
    let aggregation_enum = parse_aggregation(aggregation)?;
    validate_table_controls_scope(aggregation_enum, limit, sort_column, sort_order)?;
    let sort_order_enum = sort_order.map(parse_sort_order).transpose()?;

    eprintln!(
        "{}",
        format!("Fetching entities data for '{entity_type}'...").dimmed()
    );

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| {
        let normalized_entity_type = normalized_entity_type.clone();
        let validated_columns = validated_columns.clone();
        let validated_group_by = validated_group_by.clone();
        let validated_filters = clone_filters(&validated_filters);
        async move {
            let api = ServiceCatalogApi::new(&target.client);
            let params = DataParams {
                start: start_epoch,
                end: end_epoch,
                columns: &validated_columns,
                group_by: &validated_group_by,
                filters: &validated_filters,
                data_aggregation_type: aggregation_enum,
                limit,
                sort_column,
                sort_order: sort_order_enum,
            };
            Ok(api.entities_data(&normalized_entity_type, &params).await?)
        }
    })
    .await;

    let results = report_errors_and_collect_successes(per_profile)?;
    render_entities_data_results(results, include_profile, output)
}

/// `cx service-catalog entity-data <entity-type> <entity-id>` - column data for one entity.
#[allow(clippy::too_many_arguments)]
pub async fn run_entity_data(
    targets: &[Arc<ExecutionTarget>],
    entity_type: &str,
    entity_id: &str,
    start: &str,
    end: &str,
    columns: &[String],
    group_by: &[String],
    filters: &[String],
    aggregation: &str,
    output: OutputFormat,
) -> Result<()> {
    let normalized_entity_type = normalize_entity_type(entity_type)?;
    let entity_id = require_non_empty(entity_id, "entity id")?.to_string();
    let (start_epoch, end_epoch) = parse_time_range(start, end)?;
    let validated_columns = validate_columns(columns)?;
    let validated_group_by = validate_group_by(group_by)?;
    let validated_filters = parse_filters(filters)?;
    let aggregation_enum = parse_aggregation(aggregation)?;

    eprintln!(
        "{}",
        format!("Fetching entity data for '{entity_id}' ({entity_type})...").dimmed()
    );

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| {
        let normalized_entity_type = normalized_entity_type.clone();
        let entity_id = entity_id.clone();
        let validated_columns = validated_columns.clone();
        let validated_group_by = validated_group_by.clone();
        let validated_filters = clone_filters(&validated_filters);
        async move {
            let api = ServiceCatalogApi::new(&target.client);
            let params = DataParams {
                start: start_epoch,
                end: end_epoch,
                columns: &validated_columns,
                group_by: &validated_group_by,
                filters: &validated_filters,
                data_aggregation_type: aggregation_enum,
                limit: None,
                sort_column: None,
                sort_order: None,
            };
            Ok(api
                .entity_data(&normalized_entity_type, &entity_id, &params)
                .await?)
        }
    })
    .await;

    let results = report_errors_and_collect_successes(per_profile)?;
    render_entities_data_results(results, include_profile, output)
}

// ── Entity type / aggregation / sort-order enum mapping ─────────────────────────

const DATA_AGGREGATION_TABLE: &str = "DATA_AGGREGATION_TYPE_TABLE";
const DATA_AGGREGATION_TIMESERIES: &str = "DATA_AGGREGATION_TYPE_TIMESERIES";
const SORT_ORDER_ASC: &str = "SORT_ORDER_ASCENDING";
const SORT_ORDER_DESC: &str = "SORT_ORDER_DESCENDING";

fn parse_aggregation(aggregation: &str) -> Result<&'static str> {
    match aggregation.trim().to_lowercase().as_str() {
        "table" => Ok(DATA_AGGREGATION_TABLE),
        "timeseries" => Ok(DATA_AGGREGATION_TIMESERIES),
        other => bail!("unknown --aggregation '{other}'; allowed: table, timeseries"),
    }
}

fn parse_sort_order(sort_order: &str) -> Result<&'static str> {
    match sort_order.trim().to_lowercase().as_str() {
        "asc" => Ok(SORT_ORDER_ASC),
        "desc" => Ok(SORT_ORDER_DESC),
        other => bail!("unknown --sort-order '{other}'; allowed: asc, desc"),
    }
}

/// `--limit`/`--sort-column`/`--sort-order` are TABLE-only: the backend silently
/// ignores them for TIMESERIES (CX-52920), so the CLI rejects the combination
/// up front rather than sending a request whose flags are quietly dropped.
fn validate_table_controls_scope(
    aggregation: &str,
    limit: Option<i32>,
    sort_column: Option<&str>,
    sort_order: Option<&str>,
) -> Result<()> {
    if aggregation == DATA_AGGREGATION_TIMESERIES
        && (limit.is_some() || sort_column.is_some() || sort_order.is_some())
    {
        bail!(
            "--limit, --sort-column, and --sort-order only apply to --aggregation table; \
             the backend ignores them for timeseries, so they must be omitted rather than \
             silently dropped"
        );
    }
    Ok(())
}

// ── Validation helpers ────────────────────────────────────────────────────────

fn require_non_empty<'v>(value: &'v str, field_name: &str) -> Result<&'v str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{field_name} must not be empty");
    }
    Ok(trimmed)
}

fn parse_time_range(start: &str, end: &str) -> Result<(i64, i64)> {
    let start_epoch = crate::time::parse_timestamp_epoch_seconds(start)?;
    let end_epoch = crate::time::parse_timestamp_epoch_seconds(end)?;
    if end_epoch <= start_epoch {
        bail!("--end ({end}) must be strictly after --start ({start})");
    }
    Ok((start_epoch, end_epoch))
}

/// Discover valid column ids with `cx service-catalog schema <entity-type>` first -
/// the API rejects unknown ones anyway, so this only catches the empty case client-side.
fn validate_columns(columns: &[String]) -> Result<Vec<String>> {
    if columns.is_empty() {
        bail!(
            "--column is required and must be non-empty; run \
             `cx service-catalog schema <entity-type>` first to discover valid column ids"
        );
    }
    let trimmed: Vec<String> = columns.iter().map(|c| c.trim().to_string()).collect();
    if trimmed.iter().any(|c| c.is_empty()) {
        bail!("--column values must not be empty");
    }
    Ok(trimmed)
}

fn validate_group_by(group_by: &[String]) -> Result<Vec<String>> {
    let trimmed: Vec<String> = group_by.iter().map(|g| g.trim().to_string()).collect();
    if trimmed.iter().any(|g| g.is_empty()) {
        bail!("--group-by values must not be empty");
    }
    Ok(trimmed)
}

/// Parses repeatable `--filter label=value1,value2` flags into `ApmFilter` inputs.
///
/// Each `label` may be given at most once; combine multiple values for the same
/// label with commas rather than repeating `--filter` - repeating a label cannot
/// mean "either value" once combined with other filters (they all AND together).
fn parse_filters(filters: &[String]) -> Result<Vec<ApmFilterInput>> {
    let mut parsed: Vec<ApmFilterInput> = Vec::new();

    for raw in filters {
        let Some((label_name, values)) = raw.split_once('=') else {
            bail!("invalid --filter '{raw}': expected label=value1,value2,...");
        };
        let label_name = label_name.trim();
        if label_name.is_empty() {
            bail!("invalid --filter '{raw}': label name must not be empty");
        }
        let label_values: Vec<String> = values
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
            .collect();
        if label_values.is_empty() {
            bail!("invalid --filter '{raw}': must have at least one non-empty value");
        }
        if parsed.iter().any(|f| f.label_name == label_name) {
            bail!(
                "--filter label '{label_name}' given more than once; combine values with \
                 commas instead (--filter {label_name}=a,b) - filters AND together, so \
                 repeating a label cannot mean \"either value\""
            );
        }
        parsed.push(ApmFilterInput {
            label_name: label_name.to_string(),
            label_values,
        });
    }

    Ok(parsed)
}

fn clone_filters(filters: &[ApmFilterInput]) -> Vec<ApmFilterInput> {
    filters
        .iter()
        .map(|f| ApmFilterInput {
            label_name: f.label_name.clone(),
            label_values: f.label_values.clone(),
        })
        .collect()
}

// ── EntitiesDataResult flattening ────────────────────────────────────────────────
//
// `ColumnValue` is a protobuf `oneof` with a dozen possible shapes, so JSON keeps
// whichever single field was set. These mirror the shared Python implementation in
// cx-olly's `libs/common/src/common/tools/service_catalog_tools.py`, which the MCP
// server and Olly both use for the same v2 API - malformed/empty responses are
// treated as errors rather than silently producing an empty or partial result.

/// A `ColumnValue` object: exactly one value field, plus an optional `warning`.
fn flatten_column_value(column_value: &Value) -> Result<Value> {
    let obj = column_value
        .as_object()
        .ok_or_else(|| anyhow!("malformed service-catalog column value: expected an object"))?;
    let value_keys: Vec<&String> = obj.keys().filter(|k| k.as_str() != "warning").collect();
    if value_keys.len() != 1 {
        bail!(
            "malformed service-catalog column value: expected exactly one value field, got {:?}",
            value_keys
        );
    }
    let inner = obj[value_keys[0].as_str()].clone();
    match obj.get("warning") {
        Some(warning) if !warning.is_null() => Ok(json!({ "value": inner, "warning": warning })),
        _ => Ok(inner),
    }
}

/// A `ColumnResult`: either `{"value": ColumnValue}` or `{"error": ColumnError}`.
fn flatten_column_result(column_result: &Value) -> Result<Value> {
    let obj = column_result
        .as_object()
        .ok_or_else(|| anyhow!("malformed service-catalog column result: expected an object"))?;
    let has_value = obj.contains_key("value");
    let has_error = obj.contains_key("error");
    if has_value == has_error {
        bail!(
            "malformed service-catalog column result: expected exactly one of 'value' or 'error'"
        );
    }
    if has_value {
        return flatten_column_value(&obj["value"]);
    }
    let error = &obj["error"];
    let message = error
        .get("message")
        .and_then(|v| v.as_str())
        .or_else(|| error.get("code").and_then(|v| v.as_str()))
        .unwrap_or("unknown error");
    Ok(json!({ "error": message }))
}

fn flatten_table_row(row: &TableRow) -> Result<Value> {
    let mut flat = serde_json::Map::new();
    for (key, value) in &row.identity {
        flat.insert(key.clone(), json!(value));
    }
    for (column_id, column_result) in &row.values {
        flat.insert(column_id.clone(), flatten_column_result(column_result)?);
    }
    Ok(Value::Object(flat))
}

/// Flattens one profile's `EntitiesDataResult` into `{rows, columns}` (table) or
/// `{series, columns, total_series_count}` (timeseries). An empty/unrecognized
/// response (neither `table` nor `timeseries` set) is an error, not a silent `[]`.
fn format_entities_data_response(result: EntitiesDataResult) -> Result<Value> {
    if let Some(table) = result.table {
        let rows: Result<Vec<Value>> = table.rows.iter().map(flatten_table_row).collect();
        return Ok(json!({
            "rows": rows?,
            "columns": table.columns.iter().map(column_metadata_to_json).collect::<Vec<_>>(),
        }));
    }
    if let Some(timeseries) = result.timeseries {
        return Ok(json!({
            "series": timeseries.series,
            "columns": timeseries.columns.iter().map(column_metadata_to_json).collect::<Vec<_>>(),
            "total_series_count": timeseries.total_series_count,
        }));
    }
    bail!(
        "service-catalog returned an empty or unrecognized response \
         (neither table nor timeseries data)"
    );
}

/// Merges and renders `data`/`entity-data` results across profiles.
///
/// Table responses merge rows (tagging each with `profile` when multi-profile,
/// same convention as `resource_to_json`); timeseries responses merge series.
/// Columns are taken from the first successful profile - callers request the
/// same `--column` set per profile, so they are expected to match.
///
/// Formatting (not just the HTTP call) can fail per profile - a malformed
/// `ColumnResult` in just one profile's payload must not discard another
/// profile's perfectly good rows, so formatting failures are routed through
/// the same `report_errors_and_collect_successes` semantics as the HTTP
/// fan-out: total failure bails, partial failure prints the bad profile and
/// renders the survivors.
fn render_entities_data_results(
    results: Vec<(String, EntitiesDataResult)>,
    include_profile: bool,
    output: OutputFormat,
) -> Result<()> {
    let per_profile_formatted: Vec<(String, Result<Value>)> = results
        .into_iter()
        .map(|(profile, result)| (profile, format_entities_data_response(result)))
        .collect();
    let formatted = report_errors_and_collect_successes(per_profile_formatted)?;

    let is_timeseries = formatted
        .first()
        .map(|(_, v)| v.get("series").is_some())
        .unwrap_or(false);

    let columns = formatted
        .first()
        .and_then(|(_, v)| v.get("columns"))
        .cloned()
        .unwrap_or_else(|| json!([]));

    if is_timeseries {
        let mut series: Vec<Value> = Vec::new();
        let mut total_series_count: i64 = 0;
        for (profile, val) in &formatted {
            for item in val
                .get("series")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
            {
                series.push(tag_profile(item, include_profile, profile));
            }
            total_series_count += val
                .get("total_series_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
        }
        let envelope = json!({
            "series": series,
            "columns": columns,
            "total_series_count": total_series_count,
        });
        return render_data_envelope(output, &envelope, include_profile);
    }

    let mut rows: Vec<Value> = Vec::new();
    for (profile, val) in &formatted {
        for item in val
            .get("rows")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            rows.push(tag_profile(item, include_profile, profile));
        }
    }
    let envelope = json!({ "rows": rows, "columns": columns });
    render_data_envelope(output, &envelope, include_profile)
}

fn render_data_envelope(
    output: OutputFormat,
    envelope: &Value,
    include_profile: bool,
) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json_auto(std::slice::from_ref(envelope)),
        OutputFormat::Toon => {
            let encoded =
                toon_encode(envelope).map_err(|e| anyhow!("TOON encoding failed: {e}"))?;
            println!("{encoded}");
            Ok(())
        }
        OutputFormat::Text => {
            if let Some(series) = envelope.get("series").and_then(|v| v.as_array()) {
                if series.is_empty() {
                    render::print_no_results("No timeseries data found.");
                    return Ok(());
                }
                // Timeseries datapoints are nested and variable-shaped - printed as
                // JSON per series rather than forced into a flat table, same as the
                // dataprime pipeline's aggregate-query fallback.
                for item in series {
                    println!("{}", serde_json::to_string_pretty(item)?);
                }
                return Ok(());
            }

            let rows = envelope
                .get("rows")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if rows.is_empty() {
                render::print_no_results("No data found.");
                return Ok(());
            }
            render_table_rows(&rows, include_profile);
            Ok(())
        }
    }
}

/// Renders flattened table rows with dynamically-discovered headers: the union
/// of every row's keys (identity fields plus requested columns), `profile`
/// pulled out to the front when multi-profile. Column sets can vary slightly
/// row-to-row (e.g. a column present for one entity but not another), so the
/// header is a union rather than just the first row's keys.
fn render_table_rows(rows: &[Value], include_profile: bool) {
    let mut headers: Vec<String> = Vec::new();
    for row in rows {
        if let Some(obj) = row.as_object() {
            for key in obj.keys() {
                if key != "profile" && !headers.contains(key) {
                    headers.push(key.clone());
                }
            }
        }
    }

    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            let profile = row
                .get("profile")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mut record = vec![profile];
            for key in &headers {
                record.push(display_json_or_dash(row.get(key)));
            }
            record
        })
        .collect();

    let header_refs: Vec<&str> = headers.iter().map(String::as_str).collect();
    render::render_table(&header_refs, table_rows, include_profile);
}

// ── JSON row builders ────────────────────────────────────────────────────────────

fn render_machine_rows(output: OutputFormat, rows: &[Value]) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json(rows),
        OutputFormat::Toon => render::render_toon(rows),
        OutputFormat::Text => {
            unreachable!("callers render text themselves; only Json/Toon reach here")
        }
    }
}

fn tag_profile(mut v: Value, include_profile: bool, profile: &str) -> Value {
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

fn entity_type_info_to_json(item: &EntityTypeInfo, include_profile: bool, profile: &str) -> Value {
    let v = json!({
        "entity_type": item.entity_type,
        "id": item.id,
        "display_name": item.display_name,
        "description": item.description,
    });
    tag_profile(v, include_profile, profile)
}

fn column_metadata_to_json(c: &ColumnMetadata) -> Value {
    json!({
        "id": c.id,
        "display_name": c.display_name,
        "unit": c.unit,
    })
}

fn entity_type_metadata_to_json(m: &EntityTypeMetadata) -> Value {
    json!({
        "entity_id": m.entity_id,
        "display_name": m.display_name,
        "description": m.description,
        "columns": m.columns.iter().map(column_metadata_to_json).collect::<Vec<_>>(),
        "default_columns": m.default_columns,
        "groupable_labels": m.groupable_labels,
        "group_by_limit": m.group_by_limit,
        "default_group_by": m.default_group_by,
        "filterable_labels": m.filterable_labels,
        "required_filters": m.required_filters,
    })
}

fn entity_item_to_json(item: &EntityItem, include_profile: bool, profile: &str) -> Value {
    let v = json!({
        "name": item.name,
        "system": item.system,
        "last_seen": item.last_seen,
        "deployments": item.deployments,
        "environments": item.environments.iter().map(|e| json!({
            "name": e.name,
            "last_seen": e.last_seen,
        })).collect::<Vec<_>>(),
    });
    tag_profile(v, include_profile, profile)
}

fn display_or_dash(value: Option<&str>) -> String {
    value.filter(|s| !s.is_empty()).unwrap_or("-").to_string()
}

fn display_json_or_dash(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "-".to_string(),
        Some(Value::String(s)) if s.is_empty() => "-".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // ── parse_aggregation / parse_sort_order ─────────────────────────────────

    #[test]
    fn parse_aggregation_accepts_known_values_case_insensitively() {
        assert_eq!(parse_aggregation("table").unwrap(), DATA_AGGREGATION_TABLE);
        assert_eq!(
            parse_aggregation("TIMESERIES").unwrap(),
            DATA_AGGREGATION_TIMESERIES
        );
    }

    #[test]
    fn parse_aggregation_rejects_unknown_value() {
        let err = parse_aggregation("chart").unwrap_err();
        assert!(err.to_string().contains("unknown --aggregation 'chart'"));
    }

    #[test]
    fn parse_sort_order_accepts_asc_and_desc() {
        assert_eq!(parse_sort_order("asc").unwrap(), SORT_ORDER_ASC);
        assert_eq!(parse_sort_order("DESC").unwrap(), SORT_ORDER_DESC);
    }

    #[test]
    fn parse_sort_order_rejects_unknown_value() {
        assert!(parse_sort_order("up").is_err());
    }

    // ── validate_table_controls_scope ────────────────────────────────────────

    #[test]
    fn table_controls_allowed_for_table_aggregation() {
        assert!(validate_table_controls_scope(
            DATA_AGGREGATION_TABLE,
            Some(10),
            Some("latency"),
            Some(SORT_ORDER_ASC)
        )
        .is_ok());
    }

    #[test]
    fn table_controls_rejected_for_timeseries_aggregation() {
        let err = validate_table_controls_scope(DATA_AGGREGATION_TIMESERIES, Some(10), None, None)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("only apply to --aggregation table"));
    }

    #[test]
    fn timeseries_without_table_controls_is_fine() {
        assert!(
            validate_table_controls_scope(DATA_AGGREGATION_TIMESERIES, None, None, None).is_ok()
        );
    }

    // ── validate_columns / validate_group_by ─────────────────────────────────

    #[test]
    fn validate_columns_rejects_empty_list() {
        let err = validate_columns(&[]).unwrap_err();
        assert!(err.to_string().contains("--column is required"));
    }

    #[test]
    fn validate_columns_trims_and_rejects_blank_entries() {
        assert_eq!(
            validate_columns(&[" latency_p99 ".to_string()]).unwrap(),
            vec!["latency_p99".to_string()]
        );
        assert!(validate_columns(&["  ".to_string()]).is_err());
    }

    #[test]
    fn validate_group_by_allows_empty() {
        assert!(validate_group_by(&[]).unwrap().is_empty());
    }

    // ── parse_filters ─────────────────────────────────────────────────────────

    #[test]
    fn parse_filters_splits_comma_separated_values() {
        let filters = parse_filters(&["environment=prod,staging".to_string()]).unwrap();
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].label_name, "environment");
        assert_eq!(filters[0].label_values, vec!["prod", "staging"]);
    }

    #[test]
    fn parse_filters_rejects_missing_equals() {
        let err = parse_filters(&["environment".to_string()]).unwrap_err();
        assert!(err.to_string().contains("expected label=value1,value2"));
    }

    #[test]
    fn parse_filters_rejects_empty_values() {
        let err = parse_filters(&["environment=".to_string()]).unwrap_err();
        assert!(err.to_string().contains("at least one non-empty value"));
    }

    #[test]
    fn parse_filters_rejects_repeated_label() {
        let err = parse_filters(&["a=1".to_string(), "a=2".to_string()]).unwrap_err();
        assert!(err.to_string().contains("given more than once"));
    }

    #[test]
    fn parse_filters_allows_distinct_labels() {
        let filters =
            parse_filters(&["environment=prod".to_string(), "team=core".to_string()]).unwrap();
        assert_eq!(filters.len(), 2);
    }

    // ── flatten_column_value / flatten_column_result ─────────────────────────

    #[test]
    fn flatten_column_value_returns_the_single_set_field() {
        let value = flatten_column_value(&json!({ "metric": 42.5 })).unwrap();
        assert_eq!(value, json!(42.5));
    }

    #[test]
    fn flatten_column_value_wraps_with_warning_when_present() {
        let value = flatten_column_value(
            &json!({ "metric": 1.0, "warning": { "code": "CODE_RATE_LIMITED" } }),
        )
        .unwrap();
        assert_eq!(value["value"], json!(1.0));
        assert_eq!(value["warning"]["code"], json!("CODE_RATE_LIMITED"));
    }

    #[test]
    fn flatten_column_value_rejects_zero_value_fields() {
        let err = flatten_column_value(&json!({ "warning": null })).unwrap_err();
        assert!(err.to_string().contains("expected exactly one value field"));
    }

    #[test]
    fn flatten_column_value_rejects_multiple_value_fields() {
        let err = flatten_column_value(&json!({ "metric": 1.0, "health": {} })).unwrap_err();
        assert!(err.to_string().contains("expected exactly one value field"));
    }

    #[test]
    fn flatten_column_result_unwraps_value() {
        let value = flatten_column_result(&json!({ "value": { "metric": 7.0 } })).unwrap();
        assert_eq!(value, json!(7.0));
    }

    #[test]
    fn flatten_column_result_maps_error_message() {
        let value = flatten_column_result(&json!({
            "error": { "code": "CODE_QUERY_TIMEOUT", "message": "took too long" }
        }))
        .unwrap();
        assert_eq!(value["error"], json!("took too long"));
    }

    #[test]
    fn flatten_column_result_falls_back_to_error_code() {
        let value =
            flatten_column_result(&json!({ "error": { "code": "CODE_RATE_LIMITED" } })).unwrap();
        assert_eq!(value["error"], json!("CODE_RATE_LIMITED"));
    }

    #[test]
    fn flatten_column_result_rejects_neither_value_nor_error() {
        let err = flatten_column_result(&json!({})).unwrap_err();
        assert!(err
            .to_string()
            .contains("expected exactly one of 'value' or 'error'"));
    }

    #[test]
    fn flatten_column_result_rejects_both_value_and_error() {
        let err = flatten_column_result(&json!({ "value": {}, "error": {} })).unwrap_err();
        assert!(err
            .to_string()
            .contains("expected exactly one of 'value' or 'error'"));
    }

    #[test]
    fn flatten_table_row_merges_identity_and_values() {
        let mut identity = BTreeMap::new();
        identity.insert("name".to_string(), "checkout".to_string());
        let mut values = BTreeMap::new();
        values.insert(
            "latency_p99".to_string(),
            json!({ "value": { "metric": 42.0 } }),
        );
        let row = TableRow { identity, values };

        let flat = flatten_table_row(&row).unwrap();
        assert_eq!(flat["name"], json!("checkout"));
        assert_eq!(flat["latency_p99"], json!(42.0));
    }

    #[test]
    fn flatten_table_row_propagates_malformed_column_errors() {
        let mut values = BTreeMap::new();
        values.insert("latency_p99".to_string(), json!({}));
        let row = TableRow {
            identity: BTreeMap::new(),
            values,
        };
        assert!(flatten_table_row(&row).is_err());
    }

    // ── format_entities_data_response ────────────────────────────────────────

    #[test]
    fn format_entities_data_response_flattens_table() {
        let result: EntitiesDataResult = serde_json::from_value(json!({
            "table": {
                "rows": [
                    {
                        "identity": { "name": "checkout" },
                        "values": { "latency_p99": { "value": { "metric": 42.0 } } }
                    }
                ],
                "columns": [{ "id": "latency_p99", "displayName": "P99 Latency" }]
            }
        }))
        .unwrap();

        let formatted = format_entities_data_response(result).unwrap();
        assert_eq!(formatted["rows"][0]["name"], json!("checkout"));
        assert_eq!(formatted["rows"][0]["latency_p99"], json!(42.0));
        assert_eq!(formatted["columns"][0]["id"], json!("latency_p99"));
    }

    #[test]
    fn format_entities_data_response_passes_through_timeseries() {
        let result: EntitiesDataResult = serde_json::from_value(json!({
            "timeseries": {
                "series": [{ "columnId": "latency_p99" }],
                "columns": [{ "id": "latency_p99" }],
                "totalSeriesCount": 1
            }
        }))
        .unwrap();

        let formatted = format_entities_data_response(result).unwrap();
        assert_eq!(formatted["series"][0]["columnId"], json!("latency_p99"));
        assert_eq!(formatted["total_series_count"], json!(1));
    }

    #[test]
    fn format_entities_data_response_rejects_empty_result() {
        let result: EntitiesDataResult = serde_json::from_value(json!({})).unwrap();
        let err = format_entities_data_response(result).unwrap_err();
        assert!(err
            .to_string()
            .contains("neither table nor timeseries data"));
    }

    // ── display helpers ──────────────────────────────────────────────────────

    #[test]
    fn display_or_dash_falls_back_on_none_and_empty() {
        assert_eq!(display_or_dash(Some("value")), "value");
        assert_eq!(display_or_dash(Some("")), "-");
        assert_eq!(display_or_dash(None), "-");
    }

    #[test]
    fn display_json_or_dash_unwraps_strings_and_stringifies_others() {
        assert_eq!(display_json_or_dash(Some(&json!("checkout"))), "checkout");
        assert_eq!(display_json_or_dash(Some(&json!(42.5))), "42.5");
        assert_eq!(display_json_or_dash(Some(&Value::Null)), "-");
        assert_eq!(display_json_or_dash(None), "-");
    }

    #[test]
    fn tag_profile_inserts_key_only_when_multi_profile() {
        let tagged = tag_profile(json!({"a": 1}), true, "prod");
        assert_eq!(tagged["profile"], "prod");
        let untagged = tag_profile(json!({"a": 1}), false, "prod");
        assert!(untagged.get("profile").is_none());
    }
}
