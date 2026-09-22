use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

pub mod api;
mod legacy;

use api::{
    BoolFilter, CategoryType, ConfigChangesParams, FieldChangeData, FieldMatch, Filter,
    FilterDescriptor, GetResourcesResponse, HealthHistoryEntry, HealthPolicyData, InfraApi,
    ListResourcesParams, Op, ResourceChangeData, ResourceData, ResourceDiffData,
    ResourceHealthHistory, ResourceTypeMapping,
};

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;

/// JSON key for the source profile when merging multi-profile infra REST rows.
const JSON_KEY_PROFILE: &str = "profile";

/// Max limit of the API reads, so the CLI refuses a longer list.
const MAX_RESOURCE_IDS: usize = 100;

#[derive(Debug, Clone, Copy)]
pub struct PageWindow {
    pub start_row: Option<i64>,
    pub end_row: Option<i64>,
}

// ── Subcommand runners ────────────────────────────────────────────────────────

/// `cx infra resources types` - list the available resource type mappings.
pub async fn run_types(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching available resource types...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| async move {
        let api = InfraApi::new(&target.client);
        Ok(api.available_types().await?)
    })
    .await;

    let mut merged: Vec<(String, ResourceTypeMapping)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for mapping in resp.resource_types {
            merged.push((profile.clone(), mapping));
        }
    }

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows: Vec<Value> = merged
                .iter()
                .map(|(profile, m)| type_mapping_to_json(m, include_profile, profile))
                .collect();
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            if merged.is_empty() {
                render::print_no_results("No resource types found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = merged
                .iter()
                .map(|(profile, m)| {
                    vec![
                        profile.clone(),
                        display_or_dash(
                            m.category_type.as_ref().and_then(|c| c.category.as_deref()),
                        ),
                        display_or_dash(
                            m.category_type
                                .as_ref()
                                .and_then(|c| c.type_name.as_deref()),
                        ),
                        display_or_dash(m.label.as_deref()),
                    ]
                })
                .collect();
            render::render_table(&["Category", "Type", "Label"], rows, include_profile);
        }
    }

    Ok(())
}

pub async fn run_filters(
    targets: &[Arc<ExecutionTarget>],
    category: Option<&str>,
    resource_type: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let category = category
        .map(|c| require_non_empty(c, "--category"))
        .transpose()?;
    let resource_type = resource_type
        .map(|t| require_non_empty(t, "--type"))
        .transpose()?;

    eprintln!("{}", "Fetching filterable attributes...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| async move {
        let api = InfraApi::new(&target.client);
        Ok(api.filters(category, resource_type).await?)
    })
    .await;

    let mut merged: Vec<(String, FilterDescriptor)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for descriptor in resp.filters {
            merged.push((profile.clone(), descriptor));
        }
    }

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows: Vec<Value> = merged
                .iter()
                .map(|(profile, f)| filter_to_json(f, include_profile, profile))
                .collect();
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            if merged.is_empty() {
                render::print_no_results("No filterable attributes found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = merged
                .iter()
                .map(|(profile, f)| {
                    vec![
                        profile.clone(),
                        display_or_dash(f.name.as_deref()),
                        display_or_dash(f.kind.as_deref()),
                        if f.wildcard { "yes" } else { "no" }.to_string(),
                        join_or_dash(&f.values),
                        join_or_dash(&format_type_pairs(&f.types)),
                    ]
                })
                .collect();
            render::render_table(
                &["Attribute", "Kind", "Wildcard", "Values", "Types"],
                rows,
                include_profile,
            );
        }
    }

    Ok(())
}

/// `cx infra resources list` - list resources, narrowed by category, type or attribute.
pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    category: Option<&str>,
    resource_type: Option<&str>,
    match_all: &[String],
    match_any: &[String],
    window: PageWindow,
    output: OutputFormat,
) -> Result<()> {
    let category = category
        .map(|c| require_non_empty(c, "--category"))
        .transpose()?;
    let resource_type = resource_type
        .map(|t| require_non_empty(t, "--type"))
        .transpose()?;
    let filter = build_filter(match_all, match_any)?;
    if category.is_none() && resource_type.is_none() && filter.is_none() {
        bail!(
            "nothing to narrow by; pass at least one of --category, --type, \
             --match-all or --match-any - `cx infra resources filters` lists \
             the attributes this tenant can filter on"
        );
    }
    window.validate()?;

    eprintln!("{}", "Fetching resources...".dimmed());

    let per_profile = fan_out(targets, |target| {
        let category = category.map(String::from);
        let resource_type = resource_type.map(String::from);
        let filter = &filter;
        async move {
            let api = InfraApi::new(&target.client);
            let params = ListResourcesParams {
                category: category.as_deref(),
                resource_type: resource_type.as_deref(),
                filter: filter.as_ref(),
                start_row: window.start_row,
                end_row: window.end_row,
            };
            Ok(api.list(&params).await?)
        }
    })
    .await;

    render_resources(
        per_profile,
        targets.len() > 1,
        resource_type.is_some(),
        output,
    )
}

/// `cx infra resources list` - the deprecated name and scope filters, which
/// still require a category and a type.
pub async fn run_list_legacy(
    targets: &[Arc<ExecutionTarget>],
    category: Option<&str>,
    resource_type: Option<&str>,
    name_filter: Option<&str>,
    scope: &[String],
    window: PageWindow,
    output: OutputFormat,
) -> Result<()> {
    let (Some(category), Some(resource_type)) = (category, resource_type) else {
        bail!("--name-filter and --scope require both --category and --type");
    };
    let category = require_non_empty(category, "--category")?;
    let resource_type = require_non_empty(resource_type, "--type")?;
    let name_filter = name_filter.map(str::trim).filter(|s| !s.is_empty());
    let scope_filters = legacy::parse_scope_filters(scope)?;
    window.validate()?;

    eprintln!("{}", "Fetching resources...".dimmed());

    let per_profile = fan_out(targets, |target| {
        let category = category.to_string();
        let resource_type = resource_type.to_string();
        let name_filter = name_filter.map(String::from);
        let scope_filters = scope_filters.clone();
        async move {
            let params = legacy::LegacyListParams {
                category: &category,
                resource_type: &resource_type,
                name_filter: name_filter.as_deref(),
                scope_filters: &scope_filters,
                start_row: window.start_row,
                end_row: window.end_row,
            };
            Ok(legacy::list(&target.client, &params).await?)
        }
    })
    .await;

    render_resources(per_profile, targets.len() > 1, true, output)
}

fn render_resources(
    per_profile: Vec<(String, Result<GetResourcesResponse>)>,
    include_profile: bool,
    type_pinned: bool,
    output: OutputFormat,
) -> Result<()> {
    let mut counts: Vec<ProfileCounts> = Vec::new();
    let mut merged: Vec<(String, ResourceData)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        counts.push(ProfileCounts {
            profile: profile.clone(),
            total_count: resp.total_count,
            returned_count: resp.resources.len(),
        });
        for resource in resp.resources {
            merged.push((profile.clone(), resource));
        }
    }
    let total_count = aggregate_total(&counts);

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows: Vec<Value> = merged
                .iter()
                .map(|(profile, r)| resource_to_json(r, include_profile, profile))
                .collect();
            let envelope = build_list_envelope(total_count, rows, &counts, include_profile);
            render_machine_envelope(output, &envelope)?;
        }
        OutputFormat::Text => {
            if merged.is_empty() {
                render::print_no_results("No resources found.");
                return Ok(());
            }
            let (headers, rows) = list_table(&merged);
            let header_refs: Vec<&str> = headers.iter().map(String::as_str).collect();
            render::render_table(&header_refs, rows, include_profile);
            eprintln!(
                "{}",
                format_count_summary(merged.len(), total_count, &counts, include_profile).dimmed()
            );
            if !type_pinned {
                eprintln!(
                    "{}",
                    "Columns are the union of the matched types; a blank cell means the resource \
                     does not carry that column. Narrow with --type for one type's set."
                        .dimmed()
                );
            }
        }
    }

    Ok(())
}

/// `cx infra resources health-history <resource-id>` - daily health status samples,
/// oldest first.
///
/// Single-profile by construction (see [`single_target`]), so this issues one
/// request and renders the response directly - no fan-out, no merge, and no
/// profile tagging.
pub async fn run_health_history(
    targets: &[Arc<ExecutionTarget>],
    resource_ids: &[String],
    output: OutputFormat,
) -> Result<()> {
    let resource_ids = require_resource_ids(resource_ids)?;
    let target = single_target(targets, "health-history")?;

    eprintln!(
        "{}",
        format!(
            "Fetching health history for {} resource(s)...",
            resource_ids.len()
        )
        .dimmed()
    );

    let results = InfraApi::new(&target.client)
        .health_history(&resource_ids)
        .await
        .with_context(|| format!("profile '{}' failed", target.profile_name))?;

    report_missing_resources(resource_ids.len(), results.len());

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows: Vec<Value> = flat_map_history(&results, |resource, entry| {
                let mut row = health_entry_to_json(entry);
                row["resource_id"] = json!(resource);
                row
            });
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            let rows: Vec<Vec<String>> = flat_map_history(&results, |resource, entry| {
                vec![
                    target.profile_name.clone(),
                    resource.to_string(),
                    display_or_dash(entry.timestamp.as_deref()),
                    display_or_dash(entry.status.as_deref()),
                ]
            });
            if rows.is_empty() {
                render::print_no_results("No health history found.");
                return Ok(());
            }
            render::render_table(&["Resource", "Timestamp", "Status"], rows, false);
        }
    }

    Ok(())
}

/// `cx infra resources raw-data <resource-id>` - the raw resource document.
///
/// Single-profile by construction (see [`single_target`]), so this issues one
/// request and renders the response directly - no fan-out, no merge, and no
/// profile tagging.
pub async fn run_raw_data(
    targets: &[Arc<ExecutionTarget>],
    resource_id: &str,
    timestamp: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let resource_id = require_non_empty(resource_id, "resource id")?;
    let timestamp = timestamp
        .map(|t| crate::time::parse_timestamp_nanos(require_non_empty(t, "--timestamp")?))
        .transpose()?;
    let target = single_target(targets, "raw-data")?;

    eprintln!(
        "{}",
        format!("Fetching raw resource data for '{resource_id}'...").dimmed()
    );

    let resp = InfraApi::new(&target.client)
        .raw_data(resource_id, timestamp.as_deref())
        .await
        .with_context(|| format!("profile '{}' failed", target.profile_name))?;
    let (raw_data, version_timestamp) = (resp.raw_data, resp.version_timestamp);

    // A 200 with null raw data means the document is cleanly missing, so note it
    // on stderr and render an empty document rather than failing.
    if raw_data.is_none() {
        eprintln!("{}", "no raw data for this resource".yellow());
    }

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let envelope = json!({
                "version_timestamp": version_timestamp,
                "raw_data": raw_data,
            });
            render_machine_envelope(output, &envelope)?;
        }
        OutputFormat::Text => {
            let results: Vec<Value> = raw_data.into_iter().collect();
            let version = |_: &Value| {
                println!("version: {}", display_or_dash(version_timestamp.as_deref()));
            };
            render::render_get_text(&results, false, "No raw data found.", Some(&version))?;
        }
    }

    Ok(())
}

/// `cx infra resources config-changes` - which of these resources changed over
/// the window, most recent first. Single-profile by construction.
pub async fn run_config_changes(
    targets: &[Arc<ExecutionTarget>],
    resource_ids: &[String],
    from: &str,
    to: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let (resource_ids, from, to) = change_window(resource_ids, from, to)?;
    let target = single_target(targets, "config-changes")?;

    eprintln!(
        "{}",
        format!(
            "Checking {} resource(s) for configuration changes...",
            resource_ids.len()
        )
        .dimmed()
    );

    let params = ConfigChangesParams {
        from: &from,
        to: &to,
        resource_ids: &resource_ids,
    };
    let resp = InfraApi::new(&target.client)
        .config_changes(&params)
        .await
        .with_context(|| format!("profile '{}' failed", target.profile_name))?;
    let results = resp.results;

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows: Vec<Value> = results.iter().map(change_to_json).collect();
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            if results.is_empty() {
                render::print_no_results("No configuration changes in this window.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = results
                .iter()
                .map(|r| {
                    vec![
                        target.profile_name.clone(),
                        display_or_dash(r.resource_id.as_deref()),
                        display_or_dash(r.source.as_deref()),
                        display_or_dash(r.outcome.as_deref()),
                        display_or_dash(r.last_change.as_deref()),
                        r.change_count
                            .map_or_else(|| "-".to_string(), |c| c.to_string()),
                    ]
                })
                .collect();
            render::render_table(
                &["Resource", "Source", "Outcome", "Last Change", "Changes"],
                rows,
                false,
            );
        }
    }

    Ok(())
}

/// `cx infra resources config-diff` - what changed, field by field. Single-profile by construction.
pub async fn run_config_diff(
    targets: &[Arc<ExecutionTarget>],
    resource_ids: &[String],
    from: &str,
    to: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let (resource_ids, from, to) = change_window(resource_ids, from, to)?;
    let target = single_target(targets, "config-diff")?;

    eprintln!(
        "{}",
        format!(
            "Comparing configurations for {} resource(s)...",
            resource_ids.len()
        )
        .dimmed()
    );

    let params = ConfigChangesParams {
        from: &from,
        to: &to,
        resource_ids: &resource_ids,
    };
    let resp = InfraApi::new(&target.client)
        .config_diff(&params)
        .await
        .with_context(|| format!("profile '{}' failed", target.profile_name))?;
    let results = resp.results;

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows: Vec<Value> = results.iter().map(diff_to_json).collect();
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            if results.is_empty() {
                render::print_no_results("No configurations to compare in this window.");
                return Ok(());
            }
            let rows = diff_table(&results, &target.profile_name);
            render::render_table(
                &["Resource", "Source", "Outcome", "Field", "Before", "After"],
                rows,
                false,
            );
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Validates the resources and window the two configuration-change endpoints
/// share. `to` defaults to now.
fn change_window<'i>(
    resource_ids: &'i [String],
    from: &str,
    to: Option<&str>,
) -> Result<(Vec<&'i str>, String, String)> {
    let resource_ids = require_resource_ids(resource_ids)?;
    let from = crate::time::parse_timestamp_nanos(require_non_empty(from, "--from")?)?;
    let to = match to {
        Some(to) => crate::time::parse_timestamp_nanos(require_non_empty(to, "--to")?)?,
        None => crate::time::parse_timestamp_nanos("now")?,
    };

    // The API refuses this too
    if to < from {
        bail!("--to ({to}) is earlier than --from ({from})");
    }
    Ok((resource_ids, from, to))
}

/// One table row per field change, with the resource repeated down the group.
fn diff_table(results: &[ResourceDiffData], profile: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for result in results {
        let head = [
            profile.to_string(),
            display_or_dash(result.resource_id.as_deref()),
            display_or_dash(result.source.as_deref()),
            display_or_dash(result.outcome.as_deref()),
        ];
        if result.changes.is_empty() {
            let mut row = head.to_vec();
            row.extend(["-".to_string(), "-".to_string(), "-".to_string()]);
            rows.push(row);
            continue;
        }
        for change in &result.changes {
            let mut row = head.to_vec();
            row.push(display_or_dash(change.field.as_deref()));
            row.push(render_change_value(&change.before));
            row.push(render_change_value(&change.after));
            rows.push(row);
        }
    }
    rows
}

/// JSON `null` prints as `null` rather than a dash, because a field set to null
/// and a field that is absent are different changes.
fn render_change_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn change_to_json(item: &ResourceChangeData) -> Value {
    json!({
        "resource_id": item.resource_id,
        "source": item.source,
        "outcome": item.outcome,
        "last_change": item.last_change,
        "change_count": item.change_count,
    })
}

fn diff_to_json(item: &ResourceDiffData) -> Value {
    let changes: Vec<Value> = item.changes.iter().map(field_change_to_json).collect();
    json!({
        "resource_id": item.resource_id,
        "source": item.source,
        "outcome": item.outcome,
        "compared_from": item.compared_from,
        "compared_to": item.compared_to,
        "changes": changes,
    })
}

fn field_change_to_json(item: &FieldChangeData) -> Value {
    json!({
        "field": item.field,
        "before": item.before,
        "after": item.after,
    })
}

/// Renders merged JSON rows for the two machine formats.
///
/// Every caller peels `OutputFormat::Text` off first and renders its own table,
/// so reaching here with `Text` is a bug in this module rather than bad input.
fn render_machine_rows(output: OutputFormat, rows: &[Value]) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json(rows),
        OutputFormat::Toon => render::render_toon(rows),
        OutputFormat::Text => {
            unreachable!("callers render text themselves; only Json/Toon reach here")
        }
    }
}

/// Per-profile row counts for one `list` invocation. `total_count` is the
/// profile's fleet-wide match count, independent of the page window.
struct ProfileCounts {
    profile: String,
    total_count: i64,
    returned_count: usize,
}

/// Sums the per-profile totals. Saturating: a fan-out across profiles whose
/// totals sum past `i64::MAX` should clamp rather than wrap into a negative.
fn aggregate_total(counts: &[ProfileCounts]) -> i64 {
    counts
        .iter()
        .fold(0i64, |acc, c| acc.saturating_add(c.total_count))
}

/// Builds the `list` result envelope.
///
/// `list` is the only infra subcommand that wraps its rows instead of emitting a
/// bare array, because `--start-row`/`--end-row` make the caller responsible for
/// paging and `total_count` is the only stop condition available to them. Fleets
/// can run to hundreds of thousands of resources, so the CLI deliberately does
/// not page on the caller's behalf - it just reports the total.
///
/// `total_count` counts every resource matching the query, not just the rows in
/// this window, so it is normally larger than `returned_count`.
///
/// Key order is meaningful: the counts precede `resources` so a consumer reading
/// a truncated stream still sees the stop condition before the row payload.
fn build_list_envelope(
    total_count: i64,
    rows: Vec<Value>,
    counts: &[ProfileCounts],
    include_profile: bool,
) -> Value {
    let mut envelope = serde_json::Map::new();
    envelope.insert("total_count".to_string(), json!(total_count));
    envelope.insert("returned_count".to_string(), json!(rows.len()));

    if include_profile {
        let per_profile: Vec<Value> = counts
            .iter()
            .map(|c| {
                json!({
                    JSON_KEY_PROFILE: c.profile,
                    "total_count": c.total_count,
                    "returned_count": c.returned_count,
                })
            })
            .collect();
        envelope.insert("counts_by_profile".to_string(), Value::Array(per_profile));
    }

    envelope.insert("resources".to_string(), Value::Array(rows));
    Value::Object(envelope)
}

/// Renders the `list` envelope for the two machine formats. `Text` is unreachable
/// for the same reason as in [`render_machine_rows`].
fn render_machine_envelope(output: OutputFormat, envelope: &Value) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json_auto(std::slice::from_ref(envelope)),
        OutputFormat::Toon => {
            let encoded =
                toon_encode(envelope).map_err(|e| anyhow!("TOON encoding failed: {e}"))?;
            println!("{encoded}");
            Ok(())
        }
        OutputFormat::Text => {
            unreachable!("callers render text themselves; only Json/Toon reach here")
        }
    }
}

/// Formats the dimmed stderr summary line under the text-mode table. When fanning
/// out, the per-profile lines show each profile's own returned-vs-total figures -
/// the window applies per profile, so each is paged against its own total, not
/// the sum.
fn format_count_summary(
    returned: usize,
    total: i64,
    counts: &[ProfileCounts],
    include_profile: bool,
) -> String {
    let mut out = format!("Showing {returned} of {total} total resources");

    if include_profile {
        for c in counts {
            out.push_str(&format!(
                "\n  {}: {} of {}",
                c.profile, c.returned_count, c.total_count
            ));
        }
    }

    out
}

/// Trims a required string input and rejects it when nothing remains, so an
/// empty `--category ""` fails fast instead of sending an empty query
/// parameter to the API.
fn require_non_empty<'v>(value: &'v str, field_name: &str) -> Result<&'v str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{field_name} must not be empty");
    }
    Ok(trimmed)
}

/// Resolves the single target the `resource_id` subcommands operate on.
///
/// Resource ids are scoped to a single team, so an id
/// resolved in one profile cannot exist in another.
/// Fanning out would query every profile with an id that only one of
/// them can answer, so refuse it outright.
fn single_target<'t>(
    targets: &'t [Arc<ExecutionTarget>],
    subcommand: &str,
) -> Result<&'t ExecutionTarget> {
    match targets {
        [target] => Ok(target),
        [] => bail!("no profile resolved for `cx infra resources {subcommand}`"),
        _ => bail!(
            "`cx infra resources {subcommand}` accepts a single profile, but {} were given; \
             a resource id is scoped to one team and cannot resolve in another profile. \
             Re-run once per profile with a single -p.",
            targets.len()
        ),
    }
}

/// Validates the `--start-row` / `--end-row` page window.
///
/// The API coerces a bad window rather than rejecting it: a negative `startRow`
/// is clamped to `0`, and a window whose end is at or before its start yields a
/// row count of `0`.
///
/// Deliberately not checked here: the service's ceiling on `startRow + rows`.
/// That is a server-side policy constant which the CLI should not mirror, and its
/// 400 already names the limit and how to get under it.
impl PageWindow {
    fn validate(self) -> Result<()> {
        let Self { start_row, end_row } = self;
        if let Some(start) = start_row {
            if start < 0 {
                bail!("--start-row must not be negative (got {start}); rows are 0-based");
            }
        }

        if let Some(end) = end_row {
            if end < 0 {
                bail!("--end-row must not be negative (got {end})");
            }
        }

        if let (Some(start), Some(end)) = (start_row, end_row) {
            if end <= start {
                bail!(
                    "--end-row ({end}) must be greater than --start-row ({start}); \
                     --end-row is exclusive, so this window selects no rows"
                );
            }
        }

        Ok(())
    }
}

fn build_filter(match_all: &[String], match_any: &[String]) -> Result<Option<Filter>> {
    let all = parse_matches(match_all, "--match-all")?;
    let any_matches = parse_matches(match_any, "--match-any")?;

    if let Some(field) = all.iter().map(|m| &m.field).find(|field| {
        any_matches
            .iter()
            .any(|m| m.field.eq_ignore_ascii_case(field))
    }) {
        bail!(
            "attribute '{field}' appears in both --match-all and --match-any; \
             an attribute belongs to one of them - require it with --match-all, \
             or make it one alternative with --match-any"
        );
    }

    let mut operands: Vec<Filter> = all
        .into_iter()
        .flat_map(|m| {
            let field = m.field;
            m.values.into_iter().map(move |value| {
                Filter::Match(FieldMatch {
                    field: field.clone(),
                    values: vec![value],
                })
            })
        })
        .collect();

    let any: Vec<Filter> = any_matches.into_iter().map(Filter::Match).collect();
    if any.len() == 1 {
        operands.extend(any);
    } else if !any.is_empty() {
        operands.push(Filter::Bool(BoolFilter {
            op: Op::Or,
            operands: any,
        }));
    }

    if operands.len() == 1 {
        return Ok(operands.pop());
    }
    Ok((!operands.is_empty()).then_some(Filter::Bool(BoolFilter {
        op: Op::And,
        operands,
    })))
}

fn parse_matches(raw: &[String], flag: &str) -> Result<Vec<FieldMatch>> {
    let mut matches: Vec<FieldMatch> = Vec::new();

    for entry in raw {
        let Some((field, values_raw)) = entry.split_once('=') else {
            bail!("invalid {flag} '{entry}': expected NAME=VALUE");
        };
        let field = field.trim();
        if field.is_empty() {
            bail!("invalid {flag} '{entry}': attribute name must not be empty");
        }
        let mut values: Vec<String> = Vec::new();
        for value in values_raw.split(',').map(str::trim) {
            if !value.is_empty() && !values.iter().any(|seen| seen == value) {
                values.push(value.to_string());
            }
        }
        if values.is_empty() {
            bail!("invalid {flag} '{entry}': value must not be empty");
        }
        if matches.iter().any(|m| m.field.eq_ignore_ascii_case(field)) {
            bail!(
                "{flag} attribute '{field}' given more than once; \
                 list its values on one flag instead - {flag} {field}=a,b"
            );
        }
        matches.push(FieldMatch {
            field: field.to_string(),
            values,
        });
    }

    Ok(matches)
}

fn filter_to_json(item: &FilterDescriptor, include_profile: bool, profile: &str) -> Value {
    let v = json!({
        "name": item.name,
        "kind": item.kind,
        "wildcard": item.wildcard,
        "values": item.values,
        "types": item
            .types
            .iter()
            .map(|t| json!({ "category": t.category, "type": t.type_name }))
            .collect::<Vec<Value>>(),
    });
    tag_profile(v, include_profile, profile)
}

fn format_type_pairs(types: &[CategoryType]) -> Vec<String> {
    types
        .iter()
        .map(|t| {
            format!(
                "{}/{}",
                display_or_dash(t.category.as_deref()),
                display_or_dash(t.type_name.as_deref())
            )
        })
        .collect()
}

fn format_health_policies(policies: &[HealthPolicyData]) -> Vec<String> {
    policies
        .iter()
        .map(|p| {
            format!(
                "{} ({})",
                display_or_dash(p.name.as_deref()),
                display_or_dash(p.status.as_deref())
            )
        })
        .collect()
}

fn join_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(", ")
    }
}

/// Builds one resource row as JSON for `json` / `toon` output after fan-out.
fn resource_to_json(item: &ResourceData, include_profile: bool, profile: &str) -> Value {
    let policies: Vec<Value> = item
        .health_policies
        .iter()
        .map(|p| json!({ "id": p.id, "name": p.name, "status": p.status }))
        .collect();
    let v = json!({
        "resource_id": item.resource_id,
        "name": item.name,
        "category": item.category,
        "type": item.type_name,
        "health_policies": policies,
        "columns": item.columns,
    });
    tag_profile(v, include_profile, profile)
}

/// Injects the profile key into a JSON row when `include_profile` is true
/// (multiple `--profile`), so merged arrays stay attributable per account;
/// text mode uses a separate table path.
fn tag_profile(mut v: Value, include_profile: bool, profile: &str) -> Value {
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert(
                JSON_KEY_PROFILE.to_string(),
                Value::String(profile.to_string()),
            );
        }
    }
    v
}

/// Builds one health-history row as JSON for `json` / `toon` output.
///
/// No profile tagging: `health-history` runs against a single profile, so there
/// is nothing to disambiguate.
fn health_entry_to_json(item: &HealthHistoryEntry) -> Value {
    json!({
        "timestamp": item.timestamp,
        "status": item.status,
    })
}

/// Builds one resource-type row as JSON for `json` / `toon` output after fan-out.
fn type_mapping_to_json(item: &ResourceTypeMapping, include_profile: bool, profile: &str) -> Value {
    let v = json!({
        "category": item.category_type.as_ref().and_then(|c| c.category.clone()),
        "type": item.category_type.as_ref().and_then(|c| c.type_name.clone()),
        "label": item.label,
    });
    tag_profile(v, include_profile, profile)
}

fn display_or_dash(value: Option<&str>) -> String {
    value.filter(|s| !s.is_empty()).unwrap_or("-").to_string()
}

fn list_table(merged: &[(String, ResourceData)]) -> (Vec<String>, Vec<Vec<String>>) {
    let resources: Vec<&ResourceData> = merged.iter().map(|(_, r)| r).collect();
    let columns = union_of_columns(&resources);

    let headers: Vec<String> = ["Resource ID", "Name", "Category", "Type", "Policies"]
        .into_iter()
        .map(String::from)
        .chain(columns.iter().cloned())
        .collect();

    let rows: Vec<Vec<String>> = merged
        .iter()
        .map(|(profile, r)| {
            let mut row = vec![
                profile.clone(),
                display_or_dash(r.resource_id.as_deref()),
                display_or_dash(display_name(r)),
                display_or_dash(r.category.as_deref()),
                display_or_dash(r.type_name.as_deref()),
                join_or_dash(&format_health_policies(&r.health_policies)),
            ];
            row.extend(
                columns
                    .iter()
                    .map(|column| r.columns.get(column).cloned().unwrap_or_default()),
            );
            row
        })
        .collect();

    (headers, rows)
}

fn union_of_columns(resources: &[&ResourceData]) -> Vec<String> {
    let mut columns: Vec<String> = Vec::new();
    for resource in resources {
        for name in resource.columns.keys() {
            if !name.eq_ignore_ascii_case("name") && !columns.contains(name) {
                columns.push(name.clone());
            }
        }
    }
    columns
}

/// The name the API matches `--name-filter` against.
///
/// The `name` field is an internal identifier, while the `Name` column holds the display name
/// that `nameFilter` actually searches. Falls back to `name` when the column is absent.
fn display_name(item: &ResourceData) -> Option<&str> {
    item.columns
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("name"))
        .map(|(_, value)| value.as_str())
        .filter(|s| !s.is_empty())
        .or(item.name.as_deref())
}

fn flat_map_history<T>(
    results: &[ResourceHealthHistory],
    mut row: impl FnMut(&str, &HealthHistoryEntry) -> T,
) -> Vec<T> {
    results
        .iter()
        .flat_map(|result| {
            let resource = result.resource_id.as_deref().unwrap_or("-");
            result
                .health_history
                .iter()
                .map(move |entry| (resource, entry))
        })
        .map(|(resource, entry)| row(resource, entry))
        .collect()
}

/// The API drops ids it cannot parse and reads duplicates once, so a short
/// answer is normal. It is reported as a count because the `resourceIds` that come back
/// are normalized, so the missing ones cannot be named reliably.
fn report_missing_resources(asked: usize, answered: usize) {
    if answered < asked {
        eprintln!(
            "{}",
            format!(
                "{} of {asked} resource(s) answered; the rest were not recognised or were repeats",
                answered
            )
            .yellow()
        );
    }
}

fn require_resource_ids(resource_ids: &[String]) -> Result<Vec<&str>> {
    if resource_ids.is_empty() {
        bail!("at least one resource id is required");
    }
    if resource_ids.len() > MAX_RESOURCE_IDS {
        bail!(
            "{} resource ids given, but the API reads at most {MAX_RESOURCE_IDS} per request \
             and drops the rest without saying so. Split the list and re-run.",
            resource_ids.len()
        );
    }
    resource_ids
        .iter()
        .map(|id| require_non_empty(id, "resource id"))
        .collect()
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn require_non_empty_trims_and_accepts_values() {
        assert_eq!(require_non_empty(" Hosts ", "--category").unwrap(), "Hosts");
    }

    #[test]
    fn require_non_empty_rejects_empty_and_whitespace() {
        let err = require_non_empty("", "--category").unwrap_err();
        assert!(err.to_string().contains("--category must not be empty"));

        let err = require_non_empty("   ", "resource id").unwrap_err();
        assert!(err.to_string().contains("resource id must not be empty"));
    }

    #[test]
    fn parse_matches_reads_one_attribute_and_one_value() {
        let matches = parse_matches(&["Region=eu-west-1".to_string()], "--match-all").unwrap();
        assert_eq!(
            matches,
            vec![FieldMatch {
                field: "Region".to_string(),
                values: vec!["eu-west-1".to_string()],
            }]
        );
    }

    #[test]
    fn parse_matches_splits_values_on_commas() {
        let matches =
            parse_matches(&["Region=eu-west-1,us-east-1".to_string()], "--match-all").unwrap();
        assert_eq!(matches[0].values, vec!["eu-west-1", "us-east-1"]);
    }

    #[test]
    fn parse_matches_dedupes_repeated_values_in_one_list() {
        let matches = parse_matches(
            &["Region=eu-west-1,us-east-1,eu-west-1".to_string()],
            "--match-all",
        )
        .unwrap();
        assert_eq!(matches[0].values, vec!["eu-west-1", "us-east-1"]);
    }

    #[test]
    fn parse_matches_trims_whitespace_around_both_sides() {
        let matches = parse_matches(
            &[" Region = eu-west-1 , us-east-1 ".to_string()],
            "--match-all",
        )
        .unwrap();
        assert_eq!(matches[0].field, "Region");
        assert_eq!(matches[0].values, vec!["eu-west-1", "us-east-1"]);
    }

    #[test]
    fn parse_matches_keeps_a_value_containing_an_equals() {
        let matches = parse_matches(&["Tag=env=prod".to_string()], "--match-all").unwrap();
        assert_eq!(matches[0].values, vec!["env=prod"]);
    }

    #[test]
    fn parse_matches_rejects_a_missing_equals() {
        let err = parse_matches(&["Region".to_string()], "--match-all").unwrap_err();
        assert!(err.to_string().contains("expected NAME=VALUE"));
    }

    #[test]
    fn parse_matches_rejects_an_empty_attribute_name() {
        let err = parse_matches(&["=eu-west-1".to_string()], "--match-all").unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn parse_matches_rejects_an_empty_value() {
        for entry in ["Region=", "Region=,", "Region= , "] {
            let err = parse_matches(&[entry.to_string()], "--match-all").unwrap_err();
            assert!(
                err.to_string().contains("value must not be empty"),
                "{entry} should be refused"
            );
        }
    }

    #[test]
    fn parse_matches_rejects_a_repeated_attribute() {
        let err = parse_matches(
            &[
                "Region=eu-west-1".to_string(),
                "Region=us-east-1".to_string(),
            ],
            "--match-all",
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'Region' given more than once"), "got: {msg}");
        assert!(msg.contains("--match-all Region=a,b"), "got: {msg}");
    }

    #[test]
    fn parse_matches_detects_a_repeat_whatever_its_case() {
        let err = parse_matches(
            &[
                "Region=eu-west-1".to_string(),
                "region=us-east-1".to_string(),
            ],
            "--match-any",
        )
        .unwrap_err();
        assert!(err.to_string().contains("given more than once"));
    }

    #[test]
    fn parse_matches_names_the_flag_it_was_given() {
        let err = parse_matches(&["Region".to_string()], "--match-any").unwrap_err();
        assert!(err.to_string().contains("--match-any"));
    }

    #[test]
    fn parse_matches_empty_input_yields_no_matches() {
        assert!(parse_matches(&[], "--match-all").unwrap().is_empty());
    }

    #[test]
    fn build_filter_is_absent_without_either_flag() {
        assert_eq!(build_filter(&[], &[]).unwrap(), None);
    }

    #[test]
    fn build_filter_collapses_a_single_attribute() {
        let filter = build_filter(&["Health=critical".to_string()], &[])
            .unwrap()
            .unwrap();
        assert_eq!(
            filter,
            Filter::Match(FieldMatch {
                field: "Health".to_string(),
                values: vec!["critical".to_string()],
            })
        );
    }

    #[test]
    fn build_filter_collapses_a_single_match_any() {
        let filter = build_filter(&[], &["Health=critical".to_string()])
            .unwrap()
            .unwrap();
        assert!(matches!(filter, Filter::Match(_)), "{filter:?}");
    }

    #[test]
    fn build_filter_ands_every_match_all() {
        let filter = build_filter(
            &[
                "Region=eu-west-1".to_string(),
                "Health=critical".to_string(),
            ],
            &[],
        )
        .unwrap()
        .unwrap();
        let Filter::Bool(BoolFilter { op, operands }) = filter else {
            panic!("expected a bool");
        };
        assert_eq!(op, Op::And);
        assert_eq!(operands.len(), 2);
        assert!(operands.iter().all(|o| matches!(o, Filter::Match(_))));
    }

    #[test]
    fn build_filter_ors_every_match_any() {
        let filter = build_filter(
            &[],
            &[
                "Name=coredns".to_string(),
                "Namespace=kube-system".to_string(),
            ],
        )
        .unwrap()
        .unwrap();
        let Filter::Bool(BoolFilter { op, operands }) = filter else {
            panic!("expected a bool");
        };
        assert_eq!(op, Op::Or);
        assert_eq!(operands.len(), 2);
    }

    #[test]
    fn build_filter_nests_the_or_group_inside_the_and() {
        let filter = build_filter(
            &["OS=Linux".to_string()],
            &[
                "Health=critical".to_string(),
                "Region=eu-west-1".to_string(),
            ],
        )
        .unwrap()
        .unwrap();

        let Filter::Bool(BoolFilter { op, operands }) = filter else {
            panic!("expected a bool");
        };
        assert_eq!(op, Op::And);
        assert_eq!(operands.len(), 2);

        assert_eq!(
            operands[0],
            Filter::Match(FieldMatch {
                field: "OS".to_string(),
                values: vec!["Linux".to_string()],
            })
        );
        let Filter::Bool(BoolFilter {
            op: inner_op,
            operands: inner,
        }) = &operands[1]
        else {
            panic!("expected the second operand to be the OR group");
        };
        assert_eq!(*inner_op, Op::Or);
        assert_eq!(inner.len(), 2);
    }

    /// Across the groups this ANDs two nodes on one field, which either matches
    /// nothing or quietly collapses to the `--match-all` value alone.
    #[test]
    fn build_filter_refuses_one_attribute_in_both_groups() {
        let err = build_filter(
            &["Region=eu-west-1".to_string()],
            &["Region=us-east-1".to_string()],
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'Region' appears in both"), "got: {msg}");
        assert!(
            msg.contains("--match-all") && msg.contains("--match-any"),
            "got: {msg}"
        );
    }

    #[test]
    fn build_filter_detects_a_cross_group_repeat_whatever_its_case() {
        let err = build_filter(
            &["Region=eu-west-1".to_string()],
            &["region=us-east-1".to_string()],
        )
        .unwrap_err();
        assert!(err.to_string().contains("appears in both"));
    }

    #[test]
    fn build_filter_ands_the_values_of_one_match_all() {
        let filter = build_filter(&["Tag=a,b".to_string()], &[])
            .unwrap()
            .unwrap();
        let Filter::Bool(BoolFilter { op, operands }) = filter else {
            panic!("expected a bool");
        };
        assert_eq!(op, Op::And);
        assert_eq!(
            operands,
            vec![
                Filter::Match(FieldMatch {
                    field: "Tag".to_string(),
                    values: vec!["a".to_string()],
                }),
                Filter::Match(FieldMatch {
                    field: "Tag".to_string(),
                    values: vec!["b".to_string()],
                }),
            ]
        );
    }

    #[test]
    fn build_filter_ors_the_values_of_one_match_any() {
        let filter = build_filter(&[], &["Tag=a,b".to_string()])
            .unwrap()
            .unwrap();
        assert_eq!(
            filter,
            Filter::Match(FieldMatch {
                field: "Tag".to_string(),
                values: vec!["a".to_string(), "b".to_string()],
            })
        );
    }

    #[test]
    fn build_filter_expands_each_match_all_attribute_independently() {
        let filter = build_filter(&["Tag=a,b".to_string(), "OS=linux".to_string()], &[])
            .unwrap()
            .unwrap();
        let Filter::Bool(BoolFilter { op, operands }) = filter else {
            panic!("expected a bool");
        };
        assert_eq!(op, Op::And);
        assert_eq!(operands.len(), 3);
        assert!(operands.iter().all(|o| matches!(o, Filter::Match(_))));
    }

    #[test]
    fn build_filter_keeps_the_groups_distinct() {
        let filter = build_filter(&["Tag=a,b".to_string()], &["Region=eu,us".to_string()])
            .unwrap()
            .unwrap();
        let Filter::Bool(BoolFilter { op, operands }) = filter else {
            panic!("expected a bool");
        };
        assert_eq!(op, Op::And);
        assert_eq!(
            operands,
            vec![
                Filter::Match(FieldMatch {
                    field: "Tag".to_string(),
                    values: vec!["a".to_string()],
                }),
                Filter::Match(FieldMatch {
                    field: "Tag".to_string(),
                    values: vec!["b".to_string()],
                }),
                Filter::Match(FieldMatch {
                    field: "Region".to_string(),
                    values: vec!["eu".to_string(), "us".to_string()],
                }),
            ]
        );
    }

    #[test]
    fn build_filter_propagates_a_parse_error() {
        assert!(build_filter(&["Region".to_string()], &[]).is_err());
        assert!(build_filter(&[], &["Region".to_string()]).is_err());
    }

    // ── validate_page_window ─────────────────────────────────────────────────

    #[test]
    fn page_window_accepts_an_ascending_window_and_open_ends() {
        assert!(PageWindow {
            start_row: None,
            end_row: None
        }
        .validate()
        .is_ok());
        assert!(PageWindow {
            start_row: Some(0),
            end_row: Some(100)
        }
        .validate()
        .is_ok());
        assert!(PageWindow {
            start_row: Some(100),
            end_row: Some(200)
        }
        .validate()
        .is_ok());
        assert!(PageWindow {
            start_row: Some(100),
            end_row: None
        }
        .validate()
        .is_ok());
        assert!(PageWindow {
            start_row: None,
            end_row: Some(50)
        }
        .validate()
        .is_ok());
        assert!(PageWindow {
            start_row: Some(0),
            end_row: Some(1)
        }
        .validate()
        .is_ok());
    }

    /// The API clamps a negative `startRow` to 0 and returns 200, so without this
    /// check the caller silently gets the first window instead of an error.
    #[test]
    fn page_window_rejects_a_negative_start_row() {
        let err = PageWindow {
            start_row: Some(-5),
            end_row: None,
        }
        .validate()
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--start-row must not be negative"),
            "got: {msg}"
        );
        assert!(msg.contains("-5"), "got: {msg}");
    }

    #[test]
    fn page_window_rejects_a_negative_end_row() {
        let err = PageWindow {
            start_row: None,
            end_row: Some(-1),
        }
        .validate()
        .unwrap_err();
        assert!(err.to_string().contains("--end-row must not be negative"));
    }

    /// An inverted window yields a row count of 0 server-side, so the caller sees
    /// an empty result set and can easily read it as "no such resources".
    #[test]
    fn page_window_rejects_an_inverted_window() {
        let err = PageWindow {
            start_row: Some(200),
            end_row: Some(100),
        }
        .validate()
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("must be greater than"), "got: {msg}");
        assert!(msg.contains("100") && msg.contains("200"), "got: {msg}");
    }

    /// `--end-row` is exclusive, so an equal start and end can only ever return
    /// nothing - always a mistake rather than a meaningful request.
    #[test]
    fn page_window_rejects_an_empty_window() {
        let err = PageWindow {
            start_row: Some(100),
            end_row: Some(100),
        }
        .validate()
        .unwrap_err();
        assert!(err.to_string().contains("selects no rows"));
    }

    /// The repeat check must not reject distinct keys that share a value.
    #[test]
    fn tag_profile_inserts_key_only_when_multi_profile() {
        let tagged = tag_profile(json!({"a": 1}), true, "prod");
        assert_eq!(tagged["profile"], "prod");

        let untagged = tag_profile(json!({"a": 1}), false, "prod");
        assert!(untagged.get("profile").is_none());
    }

    // ── aggregate_total ──────────────────────────────────────────────────────

    #[test]
    fn aggregate_total_sums_profile_totals() {
        let counts = counts(&[("prod", 150, 100), ("staging", 90, 90)]);
        assert_eq!(aggregate_total(&counts), 240);
    }

    #[test]
    fn aggregate_total_of_no_profiles_is_zero() {
        assert_eq!(aggregate_total(&[]), 0);
    }

    /// Saturating rather than wrapping: a fan-out whose totals exceed `i64::MAX`
    /// must clamp, not flip to a negative "total".
    #[test]
    fn aggregate_total_saturates_instead_of_overflowing() {
        let counts = counts(&[("a", i64::MAX, 1), ("b", 1, 1)]);
        assert_eq!(aggregate_total(&counts), i64::MAX);
    }

    // ── build_list_envelope ──────────────────────────────────────────────────

    #[test]
    fn envelope_reports_total_and_returned_counts() {
        let counts = counts(&[("prod", 512_000, 100)]);
        let envelope = build_list_envelope(512_000, rows(100), &counts, false);

        assert_eq!(envelope["total_count"], 512_000);
        assert_eq!(envelope["returned_count"], 100);
        assert_eq!(envelope["resources"].as_array().unwrap().len(), 100);
    }

    /// The counts must precede `resources` so a consumer reading a truncated
    /// stream still sees its stop condition before the row payload.
    #[test]
    fn envelope_orders_counts_before_resources() {
        let counts = counts(&[("prod", 5, 5)]);
        let envelope = build_list_envelope(5, rows(5), &counts, false);
        let keys: Vec<&str> = envelope
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["total_count", "returned_count", "resources"]);
    }

    /// `returned_count` tracks the rows actually emitted, so a caller can tell a
    /// partial window from a complete one by comparing it against `total_count`.
    #[test]
    fn envelope_distinguishes_a_partial_window_from_the_fleet_total() {
        let counts = counts(&[("prod", 512_000, 100)]);
        let envelope = build_list_envelope(512_000, rows(100), &counts, false);

        assert_ne!(envelope["total_count"], envelope["returned_count"]);
        assert_eq!(envelope["total_count"], 512_000);
        assert_eq!(envelope["returned_count"], 100);
    }

    /// `returned_count` must reflect the rows actually emitted, not a per-profile
    /// figure - under fan-out it is the merged row count across all profiles.
    #[test]
    fn envelope_returned_count_matches_the_emitted_row_count() {
        let counts = counts(&[("prod", 150, 100), ("staging", 90, 90)]);
        let envelope = build_list_envelope(240, rows(190), &counts, true);

        assert_eq!(envelope["returned_count"], 190);
        assert_eq!(
            envelope["returned_count"].as_u64().unwrap() as usize,
            envelope["resources"].as_array().unwrap().len()
        );
    }

    #[test]
    fn envelope_omits_per_profile_counts_for_a_single_profile() {
        let counts = counts(&[("prod", 150, 100)]);
        let envelope = build_list_envelope(150, rows(100), &counts, false);
        assert!(envelope.get("counts_by_profile").is_none());
    }

    /// The page window applies per profile, so a summed total alone cannot tell
    /// a caller which profile still has rows pending.
    #[test]
    fn envelope_breaks_counts_down_per_profile_when_fanning_out() {
        let counts = counts(&[("prod", 150, 100), ("staging", 90, 90)]);
        let envelope = build_list_envelope(240, rows(190), &counts, true);

        let per_profile = envelope["counts_by_profile"].as_array().unwrap();
        assert_eq!(per_profile.len(), 2);
        assert_eq!(per_profile[0]["profile"], "prod");
        assert_eq!(per_profile[0]["total_count"], 150);
        assert_eq!(per_profile[0]["returned_count"], 100);
        assert_eq!(per_profile[1]["profile"], "staging");
        assert_eq!(per_profile[1]["total_count"], 90);
        assert_eq!(per_profile[1]["returned_count"], 90);
    }

    #[test]
    fn envelope_for_no_results_still_reports_counts() {
        let envelope = build_list_envelope(0, rows(0), &counts(&[("prod", 0, 0)]), false);
        assert_eq!(envelope["total_count"], 0);
        assert_eq!(envelope["returned_count"], 0);
        assert!(envelope["resources"].as_array().unwrap().is_empty());
    }

    // ── format_count_summary ─────────────────────────────────────────────────

    #[test]
    fn count_summary_reports_the_total() {
        let counts = counts(&[("prod", 512_000, 100)]);
        assert_eq!(
            format_count_summary(100, 512_000, &counts, false),
            "Showing 100 of 512000 total resources"
        );
    }

    #[test]
    fn count_summary_breaks_down_per_profile_when_fanning_out() {
        let counts = counts(&[("prod", 150, 100), ("staging", 90, 90)]);
        assert_eq!(
            format_count_summary(190, 240, &counts, true),
            "Showing 190 of 240 total resources\n  prod: 100 of 150\n  staging: 90 of 90"
        );
    }

    #[test]
    fn display_or_dash_falls_back_on_none_and_empty() {
        assert_eq!(display_or_dash(Some("value")), "value");
        assert_eq!(display_or_dash(Some("")), "-");
        assert_eq!(display_or_dash(None), "-");
    }

    fn resource(name: Option<&str>, columns: &[(&str, &str)]) -> ResourceData {
        ResourceData {
            resource_id: Some("4013226:host_id=i-077a1626590913a16".to_string()),
            name: name.map(String::from),
            columns: columns
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            category: Some("Hosts".to_string()),
            type_name: Some("EC2_Instances".to_string()),
            health_policies: Vec::new(),
        }
    }

    #[test]
    fn display_name_prefers_the_name_column() {
        let r = resource(
            Some("i-077a1626590913a16"),
            &[("Name", "ip-10-109-46-236.eu-west-1.compute.internal")],
        );
        assert_eq!(
            display_name(&r),
            Some("ip-10-109-46-236.eu-west-1.compute.internal")
        );
    }

    #[test]
    fn display_name_matches_the_column_key_case_insensitively() {
        let r = resource(Some("i-abc123"), &[("name", "web-server-1")]);
        assert_eq!(display_name(&r), Some("web-server-1"));
    }

    #[test]
    fn display_name_falls_back_to_the_name_field() {
        let missing = resource(Some("i-abc123"), &[("region", "eu-west-1")]);
        assert_eq!(display_name(&missing), Some("i-abc123"));

        let empty = resource(Some("i-abc123"), &[("Name", "")]);
        assert_eq!(display_name(&empty), Some("i-abc123"));

        let neither = resource(None, &[]);
        assert_eq!(display_name(&neither), None);
    }

    fn typed_resource(category: &str, type_name: &str, columns: &[(&str, &str)]) -> ResourceData {
        let mut r = resource(Some("name"), columns);
        r.category = Some(category.to_string());
        r.type_name = Some(type_name.to_string());
        r
    }

    #[test]
    fn union_of_columns_sorts_each_row_then_appends_new_names() {
        let a = typed_resource(
            "Hosts",
            "EC2_Instances",
            &[("Region", "eu"), ("OS", "Linux")],
        );
        let b = typed_resource("Kubernetes", "Pods", &[("Namespace", "kube-system")]);
        assert_eq!(
            union_of_columns(&[&a, &b]),
            vec![
                "OS".to_string(),
                "Region".to_string(),
                "Namespace".to_string()
            ]
        );
    }

    #[test]
    fn union_of_columns_leaves_name_to_its_own_column() {
        let r = typed_resource(
            "Hosts",
            "EC2_Instances",
            &[("Name", "web-01"), ("Region", "eu")],
        );
        assert_eq!(union_of_columns(&[&r]), vec!["Region".to_string()]);
    }

    #[test]
    fn union_of_columns_does_not_repeat_a_shared_column() {
        let a = typed_resource("Hosts", "EC2_Instances", &[("Region", "eu")]);
        let b = typed_resource("Hosts", "Azure_VMs", &[("Region", "westeu")]);
        assert_eq!(union_of_columns(&[&a, &b]), vec!["Region".to_string()]);
    }

    #[test]
    fn union_of_columns_is_empty_without_rows() {
        assert!(union_of_columns(&[]).is_empty());
    }

    fn merged_row(
        category: &str,
        type_name: &str,
        columns: &[(&str, &str)],
    ) -> (String, ResourceData) {
        (
            "p".to_string(),
            typed_resource(category, type_name, columns),
        )
    }

    #[test]
    fn list_table_always_carries_the_classification_columns() {
        let (headers, rows) = list_table(&[merged_row(
            "Hosts",
            "EC2_Instances",
            &[("Region", "eu-west-1")],
        )]);
        assert_eq!(
            headers[..5],
            ["Resource ID", "Name", "Category", "Type", "Policies"]
        );
        assert_eq!(rows[0][3], "Hosts");
        assert_eq!(rows[0][4], "EC2_Instances");
    }

    #[test]
    fn list_table_has_one_shape_whatever_the_row_mix() {
        let one_type = list_table(&[merged_row("Hosts", "EC2_Instances", &[("Region", "eu")])]).0;
        let mixed = list_table(&[
            merged_row("Hosts", "EC2_Instances", &[("Region", "eu")]),
            merged_row("Hosts", "Azure_VMs", &[("Region", "westeu")]),
        ])
        .0;
        assert_eq!(one_type, mixed);
    }

    #[test]
    fn list_table_leaves_a_missing_column_blank() {
        let (headers, rows) = list_table(&[
            merged_row("Hosts", "EC2_Instances", &[("Region", "eu-west-1")]),
            merged_row("Kubernetes", "Pods", &[("Namespace", "kube-system")]),
        ]);
        assert_eq!(headers[5..], ["Region", "Namespace"]);
        assert_eq!(rows[0][6], "eu-west-1");
        assert_eq!(rows[0][7], "");
        assert_eq!(rows[1][6], "");
        assert_eq!(rows[1][7], "kube-system");
    }

    #[test]
    fn list_table_dashes_a_missing_fixed_column() {
        let mut row = typed_resource("Hosts", "EC2_Instances", &[("Region", "eu")]);
        row.category = None;
        let (_, rows) = list_table(&[("p".to_string(), row)]);
        assert_eq!(rows[0][3], "-");
        assert_eq!(rows[0][6], "eu");
    }

    /// `--timestamp` is resolved here, not by the API: the server parses with
    /// `DateTime::parse_from_rfc3339` and would reject `now-7d` outright. This
    /// pins that every form the CLI accepts leaves as something it accepts.
    #[test]
    fn every_accepted_timestamp_form_leaves_as_rfc_3339() {
        for input in [
            "now",
            "now-7d",
            "now - 3d",
            "now-1h30m",
            "now-90s",
            "now-2w",
            "2026-09-06T00:00:00Z",
            "2026-09-06T02:00:00+02:00",
            "2026-09-03T13:26:58.137128537Z",
        ] {
            let sent = crate::time::parse_timestamp_nanos(input)
                .unwrap_or_else(|e| panic!("CLI should accept {input}: {e}"));
            chrono::DateTime::parse_from_rfc3339(&sent)
                .unwrap_or_else(|e| panic!("{input} left as {sent}, which the API rejects: {e}"));
        }
    }

    /// The history is keyed to the nanosecond, so a `version_timestamp` read
    /// from one response and fed back as `--timestamp` has to name that same
    /// version rather than resolve to the one before it.
    #[test]
    fn a_version_timestamp_survives_being_fed_back() {
        let from_the_api = "2026-09-03T13:26:58.137128537Z";
        assert_eq!(
            crate::time::parse_timestamp_nanos(from_the_api).unwrap(),
            from_the_api
        );
    }

    /// Forms the API would take but the CLI does not, so the error arrives
    /// locally with a usable message rather than as a 400.
    #[test]
    fn timestamp_forms_the_cli_refuses() {
        for input in ["now+1d", "1788442018137128537", "yesterday", "7d ago", ""] {
            assert!(
                crate::time::parse_timestamp_nanos(input).is_err(),
                "{input} should be refused"
            );
        }
    }

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn require_resource_ids_trims_every_id() {
        assert_eq!(
            require_resource_ids(&ids(&[" id-1 ", "id-2"])).unwrap(),
            vec!["id-1", "id-2"]
        );
    }

    #[test]
    fn require_resource_ids_rejects_an_empty_list() {
        let err = require_resource_ids(&[]).unwrap_err();
        assert!(err.to_string().contains("at least one resource id"));
    }

    #[test]
    fn require_resource_ids_rejects_a_blank_id() {
        let err = require_resource_ids(&ids(&["id-1", "   "])).unwrap_err();
        assert!(err.to_string().contains("resource id must not be empty"));
    }

    /// Past the cap the API truncates without saying so, which would report a
    /// partial answer as a complete one.
    #[test]
    fn require_resource_ids_rejects_more_than_the_api_reads() {
        let at_cap: Vec<String> = (0..MAX_RESOURCE_IDS).map(|i| format!("id-{i}")).collect();
        assert_eq!(
            require_resource_ids(&at_cap).unwrap().len(),
            MAX_RESOURCE_IDS
        );

        let over_cap: Vec<String> = (0..MAX_RESOURCE_IDS + 1)
            .map(|i| format!("id-{i}"))
            .collect();
        let err = require_resource_ids(&over_cap).unwrap_err();
        assert!(err.to_string().contains("at most 100"), "got: {err}");
    }

    fn history(resource_id: Option<&str>, samples: &[(&str, &str)]) -> ResourceHealthHistory {
        ResourceHealthHistory {
            resource_id: resource_id.map(String::from),
            health_history: samples
                .iter()
                .map(|(timestamp, status)| HealthHistoryEntry {
                    timestamp: Some(timestamp.to_string()),
                    status: Some(status.to_string()),
                })
                .collect(),
        }
    }

    #[test]
    fn history_flattens_to_one_row_per_sample_naming_its_resource() {
        let results = [
            history(Some("id-1"), &[("2026-07-01T00:00:00Z", "Healthy")]),
            history(
                Some("id-2"),
                &[
                    ("2026-07-01T00:00:00Z", "Critical"),
                    ("2026-07-02T00:00:00Z", "Healthy"),
                ],
            ),
        ];

        let rows = flat_map_history(&results, |resource, entry| {
            (
                resource.to_string(),
                display_or_dash(entry.status.as_deref()),
            )
        });

        assert_eq!(
            rows,
            vec![
                ("id-1".to_string(), "Healthy".to_string()),
                ("id-2".to_string(), "Critical".to_string()),
                ("id-2".to_string(), "Healthy".to_string()),
            ]
        );
    }

    /// A resource the API answered for but has no samples on contributes no
    /// rows, so it must not leave a blank row behind.
    #[test]
    fn history_skips_a_resource_without_samples() {
        let results = [
            history(Some("id-1"), &[]),
            history(Some("id-2"), &[("2026-07-01T00:00:00Z", "Healthy")]),
        ];
        let rows = flat_map_history(&results, |resource, _| resource.to_string());
        assert_eq!(rows, vec!["id-2".to_string()]);
    }

    #[test]
    fn history_dashes_a_missing_resource_id() {
        let results = [history(None, &[("2026-07-01T00:00:00Z", "Healthy")])];
        let rows = flat_map_history(&results, |resource, _| resource.to_string());
        assert_eq!(rows, vec!["-".to_string()]);
    }

    fn change(field: &str, before: Value, after: Value) -> FieldChangeData {
        FieldChangeData {
            field: Some(field.to_string()),
            before,
            after,
        }
    }

    fn diff(outcome: &str, changes: Vec<FieldChangeData>) -> ResourceDiffData {
        ResourceDiffData {
            resource_id: Some("7000098:a=frontend".to_string()),
            source: Some("OTEL".to_string()),
            outcome: Some(outcome.to_string()),
            compared_from: None,
            compared_to: None,
            changes,
        }
    }

    #[test]
    fn diff_table_repeats_the_resource_down_its_changes() {
        let results = [diff(
            "changed",
            vec![
                change("spec.replicas", json!(3), json!(10)),
                change("spec.image", json!("shop:1.4.0"), json!("shop:1.5.0")),
            ],
        )];
        let rows = diff_table(&results, "p");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][1], "7000098:a=frontend");
        assert_eq!(rows[1][1], "7000098:a=frontend");
        assert_eq!(rows[0][4], "spec.replicas");
        assert_eq!(rows[1][4], "spec.image");
    }

    /// `unchanged` and `created` are answers about a resource. Dropping them
    /// would report on fewer resources than the API replied about.
    #[test]
    fn diff_table_keeps_an_outcome_that_carries_no_changes() {
        let rows = diff_table(&[diff("unchanged", vec![])], "p");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][3], "unchanged");
        assert_eq!(rows[0][4..], ["-", "-", "-"]);
    }

    /// Strings print bare so a table stays readable; everything else keeps its
    /// JSON form so a number is not confused with the string of that number.
    #[test]
    fn change_values_render_by_type() {
        assert_eq!(render_change_value(&json!("shop:1.5.0")), "shop:1.5.0");
        assert_eq!(render_change_value(&json!(10)), "10");
        assert_eq!(render_change_value(&json!(true)), "true");
        assert_eq!(
            render_change_value(&json!({ "image": "x" })),
            r#"{"image":"x"}"#
        );
    }

    /// A field set to null and a field that is absent are different changes, so
    /// null must not render as the dash that means "missing".
    #[test]
    fn a_null_change_value_renders_as_null() {
        assert_eq!(render_change_value(&Value::Null), "null");
    }

    #[test]
    fn diff_json_keeps_the_raw_types_of_both_sides() {
        let results = diff(
            "changed",
            vec![
                change("spec.replicas", json!(3), json!(10)),
                change("spec.paused", json!(false), json!(true)),
            ],
        );
        let v = diff_to_json(&results);

        assert_eq!(v["changes"][0]["before"], json!(3));
        assert_eq!(v["changes"][0]["after"], json!(10));
        assert_eq!(v["changes"][1]["before"], json!(false));
        assert_eq!(v["outcome"], "changed");
    }

    #[test]
    fn change_window_defaults_to_now_and_keeps_nanoseconds() {
        let given = ids(&["id-1"]);
        let (kept, from, to) =
            change_window(&given, "2026-09-06T11:00:00.137128537Z", None).unwrap();

        assert_eq!(kept, vec!["id-1"]);
        assert_eq!(from, "2026-09-06T11:00:00.137128537Z");
        assert!(to.ends_with('Z'), "got: {to}");
        assert!(to > from, "an unset --to should resolve to now");
    }

    /// The API refuses this too; catching it here names the flags instead of
    /// spending a request to be told.
    #[test]
    fn change_window_rejects_an_inverted_window() {
        let err = change_window(&ids(&["id-1"]), "now-1d", Some("now-7d")).unwrap_err();
        assert!(err.to_string().contains("--to"), "got: {err}");
        assert!(err.to_string().contains("--from"), "got: {err}");
    }

    #[test]
    fn change_window_accepts_a_window_of_zero_width() {
        let at = "2026-09-06T11:00:00Z";
        assert!(change_window(&ids(&["id-1"]), at, Some(at)).is_ok());
    }

    #[test]
    fn change_window_rejects_a_bad_time_and_an_over_long_id_list() {
        assert!(change_window(&ids(&["id-1"]), "half past four", None).is_err());

        let over_cap: Vec<String> = (0..MAX_RESOURCE_IDS + 1)
            .map(|i| format!("id-{i}"))
            .collect();
        let err = change_window(&over_cap, "now-1d", None).unwrap_err();
        assert!(err.to_string().contains("at most 100"), "got: {err}");
    }

    fn policy(name: &str, status: &str) -> HealthPolicyData {
        HealthPolicyData {
            id: Some("019f4b81-0350-7561-b0cb-4a6b64c73882".to_string()),
            status: Some(status.to_string()),
            name: Some(name.to_string()),
        }
    }

    #[test]
    fn health_policies_render_every_policy_with_its_status() {
        let policies = [
            policy("Deployment has unavailable replicas", "critical"),
            policy("Pod CPU utilization high", "healthy"),
        ];
        assert_eq!(
            join_or_dash(&format_health_policies(&policies)),
            "Deployment has unavailable replicas (critical), Pod CPU utilization high (healthy)"
        );
    }

    /// The API sends `""` for a policy its catalog does not resolve, which would
    /// otherwise render as an empty pair of parentheses with nothing in front.
    #[test]
    fn an_unresolved_policy_name_renders_as_a_dash() {
        assert_eq!(
            format_health_policies(&[policy("", "healthy")]),
            vec!["- (healthy)".to_string()]
        );
    }

    #[test]
    fn a_resource_with_no_policies_renders_as_a_dash() {
        assert_eq!(join_or_dash(&format_health_policies(&[])), "-");
    }

    #[test]
    fn list_table_carries_the_policies_column() {
        let mut row = typed_resource("Kubernetes", "Deployments", &[("Namespace", "shop")]);
        row.health_policies = vec![policy("Deployment has unavailable replicas", "critical")];
        let (headers, rows) = list_table(&[("p".to_string(), row)]);

        assert_eq!(headers[4], "Policies");
        assert_eq!(rows[0][5], "Deployment has unavailable replicas (critical)");
    }

    /// json and toon mirror the API, so an unresolved name stays the empty
    /// string there. Only the table substitutes a dash.
    #[test]
    fn resource_json_keeps_the_policies_as_the_api_sent_them() {
        let mut row = typed_resource("Kubernetes", "Deployments", &[]);
        row.health_policies = vec![policy("", "pending")];
        let v = resource_to_json(&row, false, "p");

        assert_eq!(v["health_policies"][0]["name"], "");
        assert_eq!(v["health_policies"][0]["status"], "pending");
        assert_eq!(
            v["health_policies"][0]["id"],
            "019f4b81-0350-7561-b0cb-4a6b64c73882"
        );
    }

    #[test]
    fn resource_json_carries_an_empty_array_when_no_policy_applies() {
        let row = typed_resource("Hosts", "EC2_Instances", &[]);
        let v = resource_to_json(&row, false, "p");
        assert_eq!(v["health_policies"], json!([]));
    }

    #[test]
    fn a_json_row_carries_the_scope_and_the_columns() {
        let item = ResourceData {
            resource_id: Some("4013226:host_id=i-077a".to_string()),
            name: Some("prod-api-01".to_string()),
            columns: BTreeMap::from([
                ("Name".to_string(), "prod-api-01".to_string()),
                ("Region".to_string(), "eu-west-1".to_string()),
            ]),
            category: Some("Hosts".to_string()),
            type_name: Some("EC2_Instances".to_string()),
        };

        assert_eq!(
            resource_to_json(&item, false, "prod"),
            json!({
                "resource_id": "4013226:host_id=i-077a",
                "name": "prod-api-01",
                "category": "Hosts",
                "type": "EC2_Instances",
                "columns": { "Name": "prod-api-01", "Region": "eu-west-1" },
            })
        );
    }

    #[test]
    fn a_json_row_is_tagged_with_its_profile_only_when_asked() {
        let item = ResourceData {
            resource_id: Some("id".to_string()),
            name: Some("n".to_string()),
            columns: BTreeMap::new(),
            category: Some("Hosts".to_string()),
            type_name: Some("EC2_Instances".to_string()),
        };

        assert_eq!(resource_to_json(&item, true, "prod")["profile"], "prod");
        assert!(resource_to_json(&item, false, "prod")
            .get("profile")
            .is_none());
    }

    fn counts(entries: &[(&str, i64, usize)]) -> Vec<ProfileCounts> {
        entries
            .iter()
            .map(|(profile, total_count, returned_count)| ProfileCounts {
                profile: profile.to_string(),
                total_count: *total_count,
                returned_count: *returned_count,
            })
            .collect()
    }

    fn rows(n: usize) -> Vec<Value> {
        (0..n)
            .map(|i| json!({ "resource_id": format!("id-{i}") }))
            .collect()
    }
}
