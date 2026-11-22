use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

pub mod api;

use api::{
    normalize_entity_type, normalize_proto_enum, short_enum_name, ApmFilterInput, ColumnMetadata,
    DataParams, EntitiesDataResult, EntitiesInsightDrilldownParams, EntitiesInsightsParams,
    EntityInsightDrilldownParams, EntityInsightsParams, EntityItem, EntityRef, EntityTypeInfo,
    EntityTypeMetadata, InsightScope, InsightSelection, ServiceCatalogApi, TableRow,
    ENTITY_TYPE_PREFIX, ENVIRONMENT_MATCH_PREFIX, INSIGHT_BREAKDOWN_SORT_PREFIX,
    INSIGHT_CATEGORY_PREFIX, INSIGHT_DIMENSION_PREFIX, INSIGHT_DRILLDOWN_SORT_PREFIX,
    INSIGHT_TYPE_PREFIX, INSIGHT_UNCHECKED_REASON_PREFIX, REQUEST_KIND_PREFIX,
    VALID_ENVIRONMENT_MATCHES, VALID_INSIGHT_BREAKDOWN_SORTS, VALID_INSIGHT_CATEGORIES,
    VALID_INSIGHT_DIMENSIONS, VALID_INSIGHT_DRILLDOWN_SORTS, VALID_INSIGHT_TYPES,
    VALID_REQUEST_KINDS,
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

/// Raw `--type` / `--category` / `--exclude-type` / `--exclude-category` values.
pub struct InsightSelectionInput<'a> {
    pub types: &'a [String],
    pub categories: &'a [String],
    pub excluded_types: &'a [String],
    pub excluded_categories: &'a [String],
}

/// `cx service-catalog insights <entity-type>...` - insights over every entity of the given types.
#[allow(clippy::too_many_arguments)]
pub async fn run_insights(
    targets: &[Arc<ExecutionTarget>],
    entity_types: &[String],
    filters: &[String],
    selection: &InsightSelectionInput<'_>,
    breakdowns: &[String],
    top_values_per_breakdown: Option<i32>,
    breakdown_sort: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    if entity_types.is_empty() {
        bail!("at least one entity type is required");
    }
    let normalized_entity_types = entity_types
        .iter()
        .map(|entity_type| normalize_entity_type(entity_type))
        .collect::<Result<Vec<_>>>()?;
    let validated_filters = parse_filters(filters)?;
    let normalized_selection = normalize_insight_selection(selection)?;
    let normalized_breakdowns = normalize_insight_enums(
        breakdowns,
        INSIGHT_DIMENSION_PREFIX,
        VALID_INSIGHT_DIMENSIONS,
        "--breakdown",
    )?;
    let breakdown_sort_enum = breakdown_sort.map(parse_breakdown_sort).transpose()?;

    eprintln!("{}", "Fetching insights...".dimmed());

    let params = EntitiesInsightsParams {
        entity_types: &normalized_entity_types,
        filters: &validated_filters,
        selection: &normalized_selection,
        breakdowns: &normalized_breakdowns,
        top_values_per_breakdown,
        breakdown_sort: breakdown_sort_enum.as_deref(),
    };
    let per_profile = fan_out(targets, |target| {
        let params = &params;
        async move {
            let api = ServiceCatalogApi::new(&target.client);
            Ok(api.entities_insights(params).await?)
        }
    })
    .await;

    let results = report_errors_and_collect_successes(per_profile)?;
    render_insight_results(results, targets.len() > 1, output, print_entities_insights)
}

/// `cx service-catalog entity-insights <entity-type> <entity-id>` - insights of one entity.
#[allow(clippy::too_many_arguments)]
pub async fn run_entity_insights(
    targets: &[Arc<ExecutionTarget>],
    entity_type: &str,
    entity_id: &str,
    system: Option<&str>,
    environments: &[String],
    selection: &InsightSelectionInput<'_>,
    top_values_per_breakdown: Option<i32>,
    breakdown_sort: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let normalized_entity_type = normalize_entity_type(entity_type)?;
    let entity_id = require_non_empty(entity_id, "entity id")?;
    let system = system
        .map(|s| require_non_empty(s, "--system"))
        .transpose()?;
    let validated_environments = trim_non_empty_values(environments, "--environment")?;
    let normalized_selection = normalize_insight_selection(selection)?;
    let breakdown_sort_enum = breakdown_sort.map(parse_breakdown_sort).transpose()?;

    eprintln!(
        "{}",
        format!("Fetching insights for '{entity_id}' ({entity_type})...").dimmed()
    );

    let params = EntityInsightsParams {
        entity: EntityRef {
            entity_type: &normalized_entity_type,
            name: entity_id,
            system,
        },
        environments: &validated_environments,
        selection: &normalized_selection,
        top_values_per_breakdown,
        breakdown_sort: breakdown_sort_enum.as_deref(),
    };
    let per_profile = fan_out(targets, |target| {
        let params = &params;
        async move {
            let api = ServiceCatalogApi::new(&target.client);
            Ok(api.entity_insights(params).await?)
        }
    })
    .await;

    let results = report_errors_and_collect_successes(per_profile)?;
    render_insight_results(results, targets.len() > 1, output, print_entity_insights)
}

/// `cx service-catalog insight-drilldown <insight-type> <entity-type>` - the entities behind one insight.
#[allow(clippy::too_many_arguments)]
pub async fn run_insight_drilldown(
    targets: &[Arc<ExecutionTarget>],
    insight_type: &str,
    entity_type: &str,
    filters: &[String],
    scope: Option<&str>,
    environment_match: Option<&str>,
    sort: Option<&str>,
    sort_order: Option<&str>,
    page_index: Option<i32>,
    page_size: i32,
    name_search: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let normalized_insight_type = normalize_insight_type(insight_type)?;
    let normalized_entity_type = normalize_entity_type(entity_type)?;
    let validated_filters = parse_filters(filters)?;
    let parsed_scope = scope
        .map(|raw| parse_scope(raw, ENTITIES_DRILLDOWN_SCOPES))
        .transpose()?;
    let environment_match_enum = environment_match
        .map(|value| {
            normalize_proto_enum(
                value,
                ENVIRONMENT_MATCH_PREFIX,
                VALID_ENVIRONMENT_MATCHES,
                "--environment-match",
            )
        })
        .transpose()?;
    let sort_enum = sort
        .map(|value| {
            normalize_proto_enum(
                value,
                INSIGHT_DRILLDOWN_SORT_PREFIX,
                VALID_INSIGHT_DRILLDOWN_SORTS,
                "--sort",
            )
        })
        .transpose()?;
    let sort_order_enum = sort_order.map(parse_sort_order).transpose()?;

    eprintln!(
        "{}",
        format!("Fetching {entity_type} entities behind insight '{insight_type}'...").dimmed()
    );

    let params = EntitiesInsightDrilldownParams {
        insight_type: &normalized_insight_type,
        entity_type: &normalized_entity_type,
        filters: &validated_filters,
        scope: parsed_scope.as_ref(),
        environment_match: environment_match_enum.as_deref(),
        sort: sort_enum.as_deref(),
        sort_order: sort_order_enum,
        page_index,
        page_size,
        name_search,
    };
    let per_profile = fan_out(targets, |target| {
        let params = &params;
        async move {
            let api = ServiceCatalogApi::new(&target.client);
            Ok(api.entities_insight_drilldown(params).await?)
        }
    })
    .await;

    let results = report_errors_and_collect_successes(per_profile)?;
    let first_row = i64::from(page_index.unwrap_or(0)) * i64::from(page_size);
    render_insight_results(results, targets.len() > 1, output, |response| {
        print_entities_insight_drilldown(response, first_row)
    })
}

/// `cx service-catalog entity-insight-drilldown <insight-type> <entity-type> <entity-id>` -
/// one entity's values of a dimension (environments by default) under one insight.
#[allow(clippy::too_many_arguments)]
pub async fn run_entity_insight_drilldown(
    targets: &[Arc<ExecutionTarget>],
    insight_type: &str,
    entity_type: &str,
    entity_id: &str,
    system: Option<&str>,
    dimension: &str,
    environments: &[String],
    scope: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let normalized_insight_type = normalize_insight_type(insight_type)?;
    let normalized_entity_type = normalize_entity_type(entity_type)?;
    let entity_id = require_non_empty(entity_id, "entity id")?;
    let system = system
        .map(|s| require_non_empty(s, "--system"))
        .transpose()?;
    let normalized_dimension = normalize_proto_enum(
        dimension,
        INSIGHT_DIMENSION_PREFIX,
        VALID_INSIGHT_DIMENSIONS,
        "--dimension",
    )?;
    let validated_environments = trim_non_empty_values(environments, "--environment")?;
    let parsed_scope = scope
        .map(|raw| parse_scope(raw, ENTITY_DRILLDOWN_SCOPES))
        .transpose()?;

    eprintln!(
        "{}",
        format!("Fetching insight '{insight_type}' for '{entity_id}' ({entity_type})...").dimmed()
    );

    let params = EntityInsightDrilldownParams {
        insight_type: &normalized_insight_type,
        entity: EntityRef {
            entity_type: &normalized_entity_type,
            name: entity_id,
            system,
        },
        dimension: &normalized_dimension,
        environments: &validated_environments,
        scope: parsed_scope.as_ref(),
    };
    let per_profile = fan_out(targets, |target| {
        let params = &params;
        async move {
            let api = ServiceCatalogApi::new(&target.client);
            Ok(api.entity_insight_drilldown(params).await?)
        }
    })
    .await;

    let results = report_errors_and_collect_successes(per_profile)?;
    render_insight_results(
        results,
        targets.len() > 1,
        output,
        print_entity_insight_drilldown,
    )
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
    trim_non_empty_values(group_by, "--group-by")
}

fn trim_non_empty_values(values: &[String], flag: &str) -> Result<Vec<String>> {
    let trimmed: Vec<String> = values.iter().map(|v| v.trim().to_string()).collect();
    if trimmed.iter().any(|v| v.is_empty()) {
        bail!("{flag} values must not be empty");
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

// ── Insights input parsing ───────────────────────────────────────────────────────

/// `--scope` kinds accepted by `insight-drilldown` and `entity-insight-drilldown`,
/// matching the `scope` oneof of each request.
const ENTITIES_DRILLDOWN_SCOPES: &[&str] = &["language", "request-kind"];
const ENTITY_DRILLDOWN_SCOPES: &[&str] =
    &["operation", "request-kind", "database", "external-services"];

fn normalize_insight_type(insight_type: &str) -> Result<String> {
    normalize_proto_enum(
        insight_type,
        INSIGHT_TYPE_PREFIX,
        VALID_INSIGHT_TYPES,
        "insight type",
    )
}

fn normalize_insight_enums(
    values: &[String],
    prefix: &str,
    valid: &[&str],
    flag: &str,
) -> Result<Vec<String>> {
    values
        .iter()
        .map(|value| normalize_proto_enum(value, prefix, valid, flag))
        .collect()
}

fn normalize_insight_selection(selection: &InsightSelectionInput<'_>) -> Result<InsightSelection> {
    let normalize_types = |values: &[String], flag: &str| {
        normalize_insight_enums(values, INSIGHT_TYPE_PREFIX, VALID_INSIGHT_TYPES, flag)
    };
    let normalize_categories = |values: &[String], flag: &str| {
        normalize_insight_enums(
            values,
            INSIGHT_CATEGORY_PREFIX,
            VALID_INSIGHT_CATEGORIES,
            flag,
        )
    };
    Ok(InsightSelection {
        types: normalize_types(selection.types, "--type")?,
        categories: normalize_categories(selection.categories, "--category")?,
        excluded_types: normalize_types(selection.excluded_types, "--exclude-type")?,
        excluded_categories: normalize_categories(
            selection.excluded_categories,
            "--exclude-category",
        )?,
    })
}

fn parse_breakdown_sort(breakdown_sort: &str) -> Result<String> {
    normalize_proto_enum(
        breakdown_sort,
        INSIGHT_BREAKDOWN_SORT_PREFIX,
        VALID_INSIGHT_BREAKDOWN_SORTS,
        "--breakdown-sort",
    )
}

/// Parses `--scope kind=value` into the request's `scope` oneof case. The text
/// output prints each insight's scope in this same syntax, so it can be pasted
/// back as a flag.
fn parse_scope(raw: &str, allowed: &[&str]) -> Result<InsightScope> {
    let (kind, value) = match raw.split_once('=') {
        Some((kind, value)) => (kind.trim(), Some(value.trim())),
        None => (raw.trim(), None),
    };
    if !allowed.contains(&kind) {
        bail!(
            "invalid --scope '{raw}': expected one of {}",
            allowed.join(", ")
        );
    }
    let required_value = || {
        value
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow!("invalid --scope '{raw}': expected {kind}=<value>"))
    };
    let scope = match kind {
        "language" => InsightScope {
            field: "language",
            value: json!({ "name": required_value()? }),
        },
        "operation" => InsightScope {
            field: "operation",
            value: json!({ "name": required_value()? }),
        },
        "request-kind" => InsightScope {
            field: "requestKind",
            value: json!({
                "kind": normalize_proto_enum(
                    required_value()?,
                    REQUEST_KIND_PREFIX,
                    VALID_REQUEST_KINDS,
                    "request kind",
                )?
            }),
        },
        "database" => {
            let (db_system, db_namespace) = required_value()?
                .split_once('/')
                .map(|(system, namespace)| (system.trim(), namespace.trim()))
                .filter(|(system, namespace)| !system.is_empty() && !namespace.is_empty())
                .ok_or_else(|| {
                    anyhow!("invalid --scope '{raw}': expected database=<db-system>/<db-namespace>")
                })?;
            InsightScope {
                field: "dependency",
                value: json!({ "database": { "dbSystem": db_system, "dbNamespace": db_namespace } }),
            }
        }
        "external-services" => {
            if value.is_some() {
                bail!("invalid --scope '{raw}': external-services takes no value");
            }
            InsightScope {
                field: "dependency",
                value: json!({ "externalServices": {} }),
            }
        }
        _ => unreachable!("kind was checked against the allowed scopes"),
    };
    Ok(scope)
}

// ── Insights rendering ───────────────────────────────────────────────────────────
//
// The insights responses stay raw JSON from `api.rs` on: json/toon output is
// the API response as-is (camelCase, proto enum names), since the insight
// payloads are nested `oneof`s with per-environment measurements that a flat
// row would lose. Text output summarizes each response as tables.

fn render_insight_results(
    results: Vec<(String, Value)>,
    include_profile: bool,
    output: OutputFormat,
    print_text: impl Fn(&Value),
) -> Result<()> {
    let tag_profiles = |results: Vec<(String, Value)>| -> Vec<Value> {
        results
            .into_iter()
            .map(|(profile, mut response)| {
                if include_profile {
                    render::tag_get_result(&mut response, &profile);
                }
                response
            })
            .collect()
    };
    match output {
        OutputFormat::Json => render::render_json_auto(&tag_profiles(results)),
        OutputFormat::Toon => render::render_toon(&tag_profiles(results)),
        OutputFormat::Text => {
            for (profile, response) in &results {
                if include_profile {
                    println!("{}", format!("[{profile}]").dimmed());
                }
                print_text(response);
            }
            Ok(())
        }
    }
}

fn print_entities_insights(response: &Value) {
    let mut insight_rows: Vec<Vec<String>> = Vec::new();
    let mut unchecked_rows: Vec<Vec<String>> = Vec::new();
    let mut coverage_lines: Vec<String> = Vec::new();
    for per_entity_type in json_array(response, "perEntityType") {
        let entity_type =
            short_enum_name(json_str(per_entity_type, "entityType"), ENTITY_TYPE_PREFIX);
        coverage_lines.extend(format_behavior_coverage(&entity_type, per_entity_type));
        for insight in json_array(per_entity_type, "insights") {
            insight_rows.push(listed_insight_row(&entity_type, insight));
        }
        for unchecked in json_array(per_entity_type, "uncheckedInsights") {
            unchecked_rows.push(unchecked_insight_row(Some(&entity_type), unchecked));
        }
    }
    for insight in json_array(response, "companyInsights") {
        insight_rows.push(listed_insight_row("company", insight));
    }
    for unchecked in json_array(response, "uncheckedCompanyInsights") {
        unchecked_rows.push(unchecked_insight_row(Some("company"), unchecked));
    }

    if insight_rows.is_empty() {
        render::print_no_results("No insights found.");
    } else {
        render::render_table(
            &[
                "Entity Type",
                "Type",
                "Category",
                "Scope",
                "Affected",
                "Title",
            ],
            insight_rows,
            false,
        );
    }
    for line in coverage_lines {
        println!("{}", line.dimmed());
    }
    print_unchecked_insights(&["Entity Type", "Type", "Reason"], unchecked_rows);
}

fn print_entity_insights(response: &Value) {
    let insight_rows: Vec<Vec<String>> = json_array(response, "insights")
        .map(|insight| {
            vec![
                String::new(),
                insight_type_short(insight),
                insight_category_short(insight),
                format_insight_scope(insight),
                json_str_list(insight, "environments"),
                display_or_dash(Some(json_str(insight, "title"))),
            ]
        })
        .collect();
    let unchecked_rows: Vec<Vec<String>> = json_array(response, "uncheckedInsights")
        .map(|unchecked| unchecked_insight_row(None, unchecked))
        .collect();

    if insight_rows.is_empty() {
        render::print_no_results("No insights found.");
    } else {
        render::render_table(
            &["Type", "Category", "Scope", "Environments", "Title"],
            insight_rows,
            false,
        );
    }
    print_unchecked_insights(&["Type", "Reason"], unchecked_rows);
}

/// `first_row` is the zero-based offset of the requested page.
fn print_entities_insight_drilldown(response: &Value, first_row: i64) {
    print_drilldown_title(response);
    let entity_rows: Vec<Vec<String>> = json_array(response, "entities")
        .map(|entity| {
            vec![
                String::new(),
                display_or_dash(entity.pointer("/entity/name").and_then(Value::as_str)),
                display_or_dash(entity.pointer("/entity/system").and_then(Value::as_str)),
                format!(
                    "{}/{}",
                    json_i64(entity, "affectedEnvironmentCount"),
                    json_i64(entity, "environmentCount")
                ),
                format!("{:.1}%", json_f64(entity, "percentage")),
                json_str_list(entity, "affectedEnvironments"),
            ]
        })
        .collect();

    if entity_rows.is_empty() {
        render::print_no_results("No affected entities found.");
    } else {
        let shown = entity_rows.len() as i64;
        render::render_table(
            &[
                "Name",
                "System",
                "Affected Envs",
                "Percentage",
                "Affected Environments",
            ],
            entity_rows,
            false,
        );
        println!(
            "{}",
            format_page_range(first_row, shown, json_i64(response, "totalEntities")).dimmed()
        );
    }
}

/// The rows on screen within the whole result, e.g. `201-300 of 977 entities`.
fn format_page_range(first_row: i64, shown: i64, total: i64) -> String {
    format!(
        "{}-{} of {total} entities",
        first_row + 1,
        first_row + shown
    )
}

/// How many service changes the deployment insights compared, when they were
/// requested: without it, no deployment insight would read as nothing regressed.
fn format_behavior_coverage(entity_type: &str, per_entity_type: &Value) -> Option<String> {
    let coverage = per_entity_type.get("behaviorComparisonCoverage")?;
    Some(format!(
        "{entity_type}: {} of {} eligible changes compared",
        json_i64(coverage, "compared"),
        json_i64(coverage, "eligibleChanges")
    ))
}

fn print_entity_insight_drilldown(response: &Value) {
    print_drilldown_title(response);
    let value_rows: Vec<Vec<String>> = json_array(response, "values")
        .map(|dimension_value| {
            vec![
                String::new(),
                display_or_dash(Some(json_str(dimension_value, "value"))),
                render::bool_display(Some(
                    dimension_value
                        .get("isAffected")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                )),
                json_str_list(dimension_value, "affectedEnvironments"),
            ]
        })
        .collect();

    if value_rows.is_empty() {
        render::print_no_results("No values found.");
    } else {
        render::render_table(
            &["Value", "Affected", "Affected Environments"],
            value_rows,
            false,
        );
    }
}

/// Both drilldown responses carry the insight itself, or an unchecked insight
/// when its source could not be read.
fn print_drilldown_title(response: &Value) {
    if let Some(title) = response.pointer("/insight/title").and_then(Value::as_str) {
        println!("{}", title.bold());
    }
    if let Some(unchecked) = response.get("uncheckedInsight") {
        println!(
            "{}",
            format!("Not checked: {}", unchecked_reason(unchecked)).yellow()
        );
    }
}

fn print_unchecked_insights(headers: &[&str], rows: Vec<Vec<String>>) {
    if rows.is_empty() {
        return;
    }
    println!();
    println!("{}", "Not checked:".bold());
    render::render_table(headers, rows, false);
}

fn listed_insight_row(entity_type: &str, insight: &Value) -> Vec<String> {
    vec![
        String::new(),
        entity_type.to_string(),
        insight_type_short(insight),
        insight_category_short(insight),
        format_insight_scope(insight),
        format_insight_affected(insight),
        display_or_dash(Some(json_str(insight, "title"))),
    ]
}

fn unchecked_insight_row(entity_type: Option<&str>, unchecked: &Value) -> Vec<String> {
    let mut row = vec![String::new()];
    row.extend(entity_type.map(String::from));
    row.push(insight_type_short(unchecked));
    row.push(unchecked_reason(unchecked));
    row
}

fn unchecked_reason(unchecked: &Value) -> String {
    match unchecked.get("description").and_then(Value::as_str) {
        Some(description) if !description.is_empty() => description.to_string(),
        _ => short_enum_name(
            json_str(unchecked, "reason"),
            INSIGHT_UNCHECKED_REASON_PREFIX,
        ),
    }
}

fn insight_type_short(insight: &Value) -> String {
    short_enum_name(json_str(insight, "type"), INSIGHT_TYPE_PREFIX)
}

fn insight_category_short(insight: &Value) -> String {
    short_enum_name(json_str(insight, "category"), INSIGHT_CATEGORY_PREFIX)
}

/// The `affected` oneof as `count/total (pct%)`. Environment and behavior
/// coverage report the entities affected in at least one environment.
fn format_insight_affected(insight: &Value) -> String {
    if let Some(cap_usage) = insight.get("capUsage") {
        return format!(
            "{}/{} cap ({:.1}%)",
            json_i64(cap_usage, "used"),
            json_i64(cap_usage, "cap"),
            json_f64(cap_usage, "percentage")
        );
    }
    let affected_count = insight
        .get("entities")
        .or_else(|| insight.pointer("/environmentCoverage/atLeastOne"))
        .or_else(|| insight.pointer("/behaviorCoverage/atLeastOne"));
    match affected_count {
        Some(count) => format!(
            "{}/{} ({:.1}%)",
            json_i64(count, "insightCount"),
            json_i64(count, "totalEntities"),
            json_f64(count, "percentage")
        ),
        None => "-".to_string(),
    }
}

/// The `scope` oneof in `--scope` syntax, e.g. `request-kind=server`.
fn format_insight_scope(insight: &Value) -> String {
    if let Some(name) = insight.pointer("/language/name").and_then(Value::as_str) {
        return format!("language={name}");
    }
    if let Some(name) = insight.pointer("/operation/name").and_then(Value::as_str) {
        return format!("operation={name}");
    }
    if let Some(kind) = insight.pointer("/requestKind/kind").and_then(Value::as_str) {
        return format!(
            "request-kind={}",
            short_enum_name(kind, REQUEST_KIND_PREFIX)
        );
    }
    if let Some(database) = insight.pointer("/dependency/database") {
        return format!(
            "database={}/{}",
            json_str(database, "dbSystem"),
            json_str(database, "dbNamespace")
        );
    }
    if insight.pointer("/dependency/externalServices").is_some() {
        return "external-services".to_string();
    }
    "-".to_string()
}

// Proto JSON omits fields holding their default value, so a missing number
// is 0, a missing string is empty and a missing list is empty.

fn json_array<'v>(value: &'v Value, key: &str) -> impl Iterator<Item = &'v Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn json_str<'v>(value: &'v Value, key: &str) -> &'v str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn json_i64(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn json_f64(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn json_str_list(value: &Value, key: &str) -> String {
    let joined = json_array(value, key)
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    display_or_dash(Some(&joined))
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

    // ── insights input parsing ───────────────────────────────────────────────

    #[test]
    fn normalize_insight_selection_names_the_offending_flag() {
        let categories = vec!["deployment".to_string()];
        let excluded_types = vec!["slow".to_string()];
        let err = normalize_insight_selection(&InsightSelectionInput {
            types: &[],
            categories: &categories,
            excluded_types: &excluded_types,
            excluded_categories: &[],
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("unknown --exclude-type 'slow'"),
            "got: {err}"
        );
    }

    #[test]
    fn normalize_insight_selection_expands_short_forms() {
        let types = vec!["no-slo".to_string()];
        let selection = normalize_insight_selection(&InsightSelectionInput {
            types: &types,
            categories: &[],
            excluded_types: &[],
            excluded_categories: &[],
        })
        .unwrap();
        assert_eq!(selection.types, vec!["INSIGHT_TYPE_NO_SLO"]);
    }

    #[test]
    fn parse_scope_maps_each_kind_to_its_oneof_case() {
        let request_kind = parse_scope("request-kind=server", ENTITY_DRILLDOWN_SCOPES).unwrap();
        assert_eq!(request_kind.field, "requestKind");
        assert_eq!(request_kind.value, json!({ "kind": "REQUEST_KIND_SERVER" }));

        let operation = parse_scope("operation=GET /cart?a=b", ENTITY_DRILLDOWN_SCOPES).unwrap();
        assert_eq!(operation.field, "operation");
        assert_eq!(operation.value, json!({ "name": "GET /cart?a=b" }));

        let database = parse_scope("database=postgresql/orders", ENTITY_DRILLDOWN_SCOPES).unwrap();
        assert_eq!(database.field, "dependency");
        assert_eq!(
            database.value,
            json!({ "database": { "dbSystem": "postgresql", "dbNamespace": "orders" } })
        );

        let external = parse_scope("external-services", ENTITY_DRILLDOWN_SCOPES).unwrap();
        assert_eq!(external.value, json!({ "externalServices": {} }));

        let language = parse_scope("language=java", ENTITIES_DRILLDOWN_SCOPES).unwrap();
        assert_eq!(language.field, "language");
        assert_eq!(language.value, json!({ "name": "java" }));
    }

    #[test]
    fn parse_scope_rejects_a_kind_the_request_does_not_take() {
        let err = parse_scope("operation=GET /cart", ENTITIES_DRILLDOWN_SCOPES).unwrap_err();
        assert!(
            err.to_string()
                .contains("expected one of language, request-kind"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_scope_rejects_missing_or_malformed_values() {
        assert!(parse_scope("operation", ENTITY_DRILLDOWN_SCOPES).is_err());
        assert!(parse_scope("database=postgresql", ENTITY_DRILLDOWN_SCOPES).is_err());
        for empty_half in ["database=/orders", "database=postgresql/", "database= / "] {
            let err = parse_scope(empty_half, ENTITY_DRILLDOWN_SCOPES).unwrap_err();
            assert!(
                err.to_string()
                    .contains("expected database=<db-system>/<db-namespace>"),
                "{empty_half}: {err}"
            );
        }
        assert!(parse_scope("request-kind=client", ENTITY_DRILLDOWN_SCOPES).is_err());
        assert!(parse_scope("external-services=x", ENTITY_DRILLDOWN_SCOPES).is_err());
    }

    // ── insights rendering helpers ───────────────────────────────────────────

    #[test]
    fn format_insight_scope_round_trips_through_parse_scope() {
        let insights = [
            json!({ "operation": { "name": "GET /cart" } }),
            json!({ "requestKind": { "kind": "REQUEST_KIND_CONSUMER" } }),
            json!({ "dependency": { "database": { "dbSystem": "redis", "dbNamespace": "0" } } }),
            json!({ "dependency": { "externalServices": {} } }),
        ];
        for insight in insights {
            let printed = format_insight_scope(&insight);
            let parsed = parse_scope(&printed, ENTITY_DRILLDOWN_SCOPES).unwrap();
            assert_eq!(insight[parsed.field], parsed.value, "scope {printed}");
        }
        assert_eq!(format_insight_scope(&json!({})), "-");
    }

    #[test]
    fn format_insight_affected_covers_each_oneof_case() {
        assert_eq!(
            format_insight_affected(&json!({
                "entities": { "insightCount": 3, "totalEntities": 12, "percentage": 25 }
            })),
            "3/12 (25.0%)"
        );
        assert_eq!(
            format_insight_affected(&json!({
                "environmentCoverage": {
                    "atLeastOne": { "insightCount": 2, "totalEntities": 4, "percentage": 50 },
                    "every": { "totalEntities": 4 }
                }
            })),
            "2/4 (50.0%)"
        );
        assert_eq!(
            format_insight_affected(&json!({
                "behaviorCoverage": { "atLeastOne": { "insightCount": 1, "totalEntities": 8, "percentage": 12.5 } }
            })),
            "1/8 (12.5%)"
        );
        assert_eq!(
            format_insight_affected(&json!({
                "capUsage": { "used": 9500, "cap": 10000, "percentage": 95 }
            })),
            "9500/10000 cap (95.0%)"
        );
        assert_eq!(format_insight_affected(&json!({})), "-");
    }

    #[test]
    fn format_page_range_counts_from_the_page_offset() {
        assert_eq!(format_page_range(0, 3, 77), "1-3 of 77 entities");
        assert_eq!(format_page_range(200, 100, 977), "201-300 of 977 entities");
    }

    #[test]
    fn format_behavior_coverage_only_when_deployment_insights_ran() {
        assert_eq!(
            format_behavior_coverage(
                "service",
                &json!({ "behaviorComparisonCoverage": { "eligibleChanges": 2000, "compared": 275 } })
            )
            .as_deref(),
            Some("service: 275 of 2000 eligible changes compared")
        );
        // Proto JSON omits zero counts.
        assert_eq!(
            format_behavior_coverage("service", &json!({ "behaviorComparisonCoverage": {} }))
                .as_deref(),
            Some("service: 0 of 0 eligible changes compared")
        );
        assert_eq!(format_behavior_coverage("service", &json!({})), None);
    }

    #[test]
    fn unchecked_reason_prefers_description_over_reason_enum() {
        assert_eq!(
            unchecked_reason(&json!({
                "reason": "INSIGHT_UNCHECKED_REASON_PERMISSION_DENIED",
                "description": "Missing SLO read permission"
            })),
            "Missing SLO read permission"
        );
        assert_eq!(
            unchecked_reason(&json!({ "reason": "INSIGHT_UNCHECKED_REASON_SOURCE_UNAVAILABLE" })),
            "source-unavailable"
        );
    }
}
