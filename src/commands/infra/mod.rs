use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

pub mod api;
mod legacy;

use api::{
    BoolFilter, CategoryType, ConfigChangesParams, FieldChangeData, FieldMatch, Filter,
    FilterDescriptor, GetEnablementResponse, GetResourcesResponse, HealthHistoryEntry,
    HealthPolicyData, InfraApi, ListResourcesParams, Op, ResourceChangeData, ResourceData,
    ResourceDiffData, ResourceHealthHistory, ResourceTypeMapping,
};

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;

/// JSON key for the source profile when merging multi-profile infra REST rows.
const JSON_KEY_PROFILE: &str = "profile";

/// Max limit of the API reads, so the CLI refuses a longer list.
const MAX_RESOURCE_IDS: usize = 100;

/// Values longer than this that the API joined with ", " are broken one item per line.
const WRAP_LIST_LENGTH: usize = 60;

#[derive(Debug, Clone, Copy)]
pub struct PageWindow {
    pub start_row: Option<i64>,
    pub end_row: Option<i64>,
}

// ── Enablement ────────────────────────────────────────────────────────────────

/// Only an explicit `enabled: false` blocks. When the check itself cannot
/// answer the command continues with a warning on stderr.
async fn check_enabled(api: &InfraApi<'_>, profile_name: &str) -> Result<()> {
    let reason = match api.enablement().await {
        Ok(GetEnablementResponse {
            enabled: Some(true),
        }) => return Ok(()),
        Ok(GetEnablementResponse {
            enabled: Some(false),
        }) => bail!("Infrastructure monitoring is not enabled for this team"),
        Ok(GetEnablementResponse { enabled: None }) => {
            "the response has no `enabled` field".to_string()
        }
        Err(e) => e.to_string(),
    };
    eprintln!(
        "{}",
        format!(
            "warning: profile '{profile_name}': could not verify that infrastructure \
             monitoring is enabled ({reason}); continuing"
        )
        .yellow()
    );
    Ok(())
}

// ── Subcommand runners ────────────────────────────────────────────────────────

/// `cx infra resources types` - list the available resource type mappings.
pub async fn run_types(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching available resource types...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| async move {
        let api = InfraApi::new(&target.client);
        check_enabled(&api, &target.profile_name).await?;
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
        check_enabled(&api, &target.profile_name).await?;
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
            check_enabled(&api, &target.profile_name).await?;
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

    render_resources(per_profile, targets.len() > 1, output)
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
            check_enabled(&InfraApi::new(&target.client), &target.profile_name).await?;
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

    render_resources(per_profile, targets.len() > 1, output)
}

fn render_resources(
    per_profile: Vec<(String, Result<GetResourcesResponse>)>,
    include_profile: bool,
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
            print!("{}", resource_blocks(&merged, include_profile));
            eprintln!(
                "{}",
                format_count_summary(merged.len(), total_count, &counts, include_profile).dimmed()
            );
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
    let target = single_target(targets, "health-history").await?;

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

    report_missing_resources(
        resource_ids.len(),
        results.len(),
        "the rest were not recognised or were repeats",
    );

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows: Vec<Value> = results.iter().map(history_to_json).collect();
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            if results.is_empty() {
                render::print_no_results("No health history found.");
                return Ok(());
            }
            let rows = history_table(&results, &target.profile_name);
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
    let target = single_target(targets, "raw-data").await?;

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
    let target = single_target(targets, "config-changes").await?;

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
    let target = single_target(targets, "config-diff").await?;

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

    report_missing_resources(
        resource_ids.len(),
        distinct_resources(&results),
        "the rest were not recognised, were repeats, or have no history around this window",
    );

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
            print!("{}", diff_blocks(&results, true));
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

/// One block per resource and source: a header, then the changed fields as a tree of
/// their paths with the before and after values under each. Paths nest deep
/// and values can be whole subtrees, so a table cannot fit them across any terminal.
fn diff_blocks(results: &[ResourceDiffData], paint: bool) -> String {
    results
        .iter()
        .map(|r| diff_block(r, paint))
        .collect::<Vec<_>>()
        .join("\n")
}

fn diff_block(result: &ResourceDiffData, paint: bool) -> String {
    let heading = display_or_dash(result.resource_id.as_deref());
    let mut out = format!(
        "{}\n",
        if paint {
            heading.bold().to_string()
        } else {
            heading
        }
    );

    let compared = match (&result.compared_from, &result.compared_to) {
        (None, None) => "-".to_string(),
        (from, to) => format!(
            "{} → {}",
            from.as_deref().map_or("-".to_string(), trim_fraction),
            to.as_deref().map_or("-".to_string(), trim_fraction)
        ),
    };
    let mut fields = vec![
        ("Source", display_or_dash(result.source.as_deref())),
        ("Outcome", display_or_dash(result.outcome.as_deref())),
        ("Compared", compared),
    ];
    if !result.changes.is_empty() {
        fields.push(("Changes", result.changes.len().to_string()));
    }
    for (key, value) in fields {
        out.push_str(&format!("  {key:<8}  {value}\n"));
    }

    if !result.changes.is_empty() {
        out.push('\n');
        push_tree(&mut out, &path_tree(&result.changes), 2, paint);
    }
    out
}

/// Drops the sub-second digits the API sends, which only widen the header.
fn trim_fraction(ts: &str) -> String {
    match (ts.find('T'), ts.find('.')) {
        (Some(t), Some(dot)) if dot > t && ts.ends_with('Z') => format!("{}Z", &ts[..dot]),
        _ => ts.to_string(),
    }
}

#[derive(Default)]
struct PathNode<'c> {
    label: String,
    change: Option<&'c FieldChangeData>,
    children: Vec<PathNode<'c>>,
}

/// Groups the changes by path, in the order the API sent them, and collapses every run
/// of single-child segments into one label so a lone deep change stays shallow.
fn path_tree(changes: &[FieldChangeData]) -> Vec<PathNode<'_>> {
    let mut root = PathNode::default();
    for change in changes {
        let mut node = &mut root;
        for segment in path_segments(change.field.as_deref().unwrap_or_default()) {
            let i = match node.children.iter().position(|c| c.label == segment) {
                Some(i) => i,
                None => {
                    node.children.push(PathNode {
                        label: segment,
                        ..PathNode::default()
                    });
                    node.children.len() - 1
                }
            };
            node = &mut node.children[i];
        }
        node.change = Some(change);
    }
    root.children.into_iter().map(collapse).collect()
}

fn collapse(mut node: PathNode<'_>) -> PathNode<'_> {
    while node.change.is_none() && node.children.len() == 1 {
        let child = node.children.remove(0);
        let separator = if child.label.starts_with('[') {
            ""
        } else {
            "."
        };
        node.label = format!("{}{separator}{}", node.label, child.label);
        node.change = child.change;
        node.children = child.children;
    }
    node.children = node.children.into_iter().map(collapse).collect();
    node
}

/// Splits a field path on `.` and on `[...]` keys. A key keeps its brackets and any dots
/// inside it, as in `labels[app.kubernetes.io/name]`.
fn path_segments(field: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in field.chars() {
        match ch {
            '[' => {
                if depth == 0 && !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                depth += 1;
                current.push(ch);
            }
            ']' if depth > 0 => {
                current.push(ch);
                depth -= 1;
                if depth == 0 {
                    segments.push(std::mem::take(&mut current));
                }
            }
            '.' if depth == 0 => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    if segments.is_empty() {
        segments.push("-".to_string());
    }
    segments
}

fn push_tree(out: &mut String, nodes: &[PathNode<'_>], indent: usize, paint: bool) {
    for node in nodes {
        out.push_str(&format!("{:indent$}{}\n", "", node.label));
        if let Some(change) = node.change {
            push_side(out, &change.before, '-', indent + 2, paint);
            push_side(out, &change.after, '+', indent + 2, paint);
        }
        push_tree(out, &node.children, indent + 2, paint);
    }
}

/// `null` is the side of an added or removed field, so it prints nothing and the
/// other side alone shows which it was.
fn push_side(out: &mut String, value: &Value, mark: char, indent: usize, paint: bool) {
    if value.is_null() {
        return;
    }
    for line in yaml_lines(value) {
        let line = format!("{mark} {line}");
        let line = match (paint, mark) {
            (false, _) => line,
            (true, '-') => line.red().to_string(),
            (true, _) => line.green().to_string(),
        };
        out.push_str(&format!("{:indent$}{line}\n", ""));
    }
}

/// A value as block-style YAML lines, two spaces per level.
fn yaml_lines(value: &Value) -> Vec<String> {
    match value {
        Value::Object(map) if !map.is_empty() => map
            .iter()
            .flat_map(|(key, v)| yaml_entry(&format!("{key}:"), v))
            .collect(),
        Value::Array(items) if !items.is_empty() => {
            items.iter().flat_map(|v| yaml_entry("-", v)).collect()
        }
        scalar => vec![yaml_scalar(scalar)],
    }
}

/// `key:` or `-` and its value: inline for a scalar, on the dash line for the first key
/// of a map in a list, and indented below otherwise.
fn yaml_entry(lead: &str, value: &Value) -> Vec<String> {
    let nested = yaml_lines(value);
    let is_container = match value {
        Value::Object(m) => !m.is_empty(),
        Value::Array(a) => !a.is_empty(),
        _ => false,
    };
    if !is_container {
        return vec![format!("{lead} {}", nested[0])];
    }
    if lead == "-" && value.is_object() {
        return nested
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                if i == 0 {
                    format!("- {line}")
                } else {
                    format!("  {line}")
                }
            })
            .collect();
    }
    std::iter::once(lead.to_string())
        .chain(nested.into_iter().map(|line| format!("  {line}")))
        .collect()
}

/// Strings print bare unless YAML would read them as something else, so a number is
/// not confused with the string of that number.
fn yaml_scalar(value: &Value) -> String {
    match value {
        Value::String(s) if yaml_needs_quotes(s) => value.to_string(),
        Value::String(s) => s.clone(),
        Value::Object(_) => "{}".to_string(),
        Value::Array(_) => "[]".to_string(),
        other => other.to_string(),
    }
}

fn yaml_needs_quotes(s: &str) -> bool {
    s.is_empty()
        || s != s.trim()
        || s.contains('\n')
        || s.contains(": ")
        || s.contains(" #")
        || matches!(s, "true" | "false" | "null" | "~")
        || s.parse::<f64>().is_ok()
        || s.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c))
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
/// Errors when that one profile's team has infrastructure monitoring disabled.
async fn single_target<'t>(
    targets: &'t [Arc<ExecutionTarget>],
    subcommand: &str,
) -> Result<&'t ExecutionTarget> {
    match targets {
        [target] => {
            check_enabled(&InfraApi::new(&target.client), &target.profile_name)
                .await
                .with_context(|| format!("profile '{}' failed", target.profile_name))?;
            Ok(target)
        }
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

fn history_to_json(item: &ResourceHealthHistory) -> Value {
    let history: Vec<Value> = item
        .health_history
        .iter()
        .map(health_entry_to_json)
        .collect();
    json!({
        "resource_id": item.resource_id,
        "health_history": history,
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

/// One block per resource, a field per line. Resources carry dozens of columns and the
/// set grows with the type, so a table cannot fit them across any terminal.
fn resource_blocks(merged: &[(String, ResourceData)], include_profile: bool) -> String {
    merged
        .iter()
        .map(|(profile, r)| resource_block(r, include_profile.then_some(profile.as_str())))
        .collect::<Vec<_>>()
        .join("\n")
}

fn resource_block(r: &ResourceData, profile: Option<&str>) -> String {
    let mut fields: Vec<(&str, Vec<String>)> = Vec::new();
    if let Some(profile) = profile {
        fields.push(("Profile", vec![profile.to_string()]));
    }
    fields.push((
        "Resource ID",
        vec![display_or_dash(r.resource_id.as_deref())],
    ));
    fields.push((
        "Type",
        vec![format!(
            "{} / {}",
            display_or_dash(r.category.as_deref()),
            display_or_dash(r.type_name.as_deref())
        )],
    ));
    let policies = format_health_policies(&r.health_policies);
    fields.push((
        "Policies",
        if policies.is_empty() {
            vec!["-".to_string()]
        } else {
            policies
        },
    ));
    fields.extend(
        r.columns
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("name"))
            .map(|(name, value)| (name.as_str(), split_long_list(value))),
    );

    let width = fields.iter().map(|(key, _)| key.len()).max().unwrap_or(0);
    let mut out = format!("{}\n", display_or_dash(display_name(r)).bold());
    for (key, lines) in &fields {
        for (i, line) in lines.iter().enumerate() {
            let key = if i == 0 { *key } else { "" };
            out.push_str(&format!("  {key:<width$}  {line}\n"));
        }
    }
    out
}

fn split_long_list(value: &str) -> Vec<String> {
    if value.len() > WRAP_LIST_LENGTH && value.contains(", ") {
        value.split(", ").map(String::from).collect()
    } else {
        vec![value.to_string()]
    }
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

/// One table row per sample, and a row of dashes for a resource with none.
fn history_table(results: &[ResourceHealthHistory], profile: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for result in results {
        let resource = display_or_dash(result.resource_id.as_deref());
        if result.health_history.is_empty() {
            rows.push(vec![
                profile.to_string(),
                resource,
                "-".to_string(),
                "-".to_string(),
            ]);
            continue;
        }
        for entry in &result.health_history {
            rows.push(vec![
                profile.to_string(),
                resource.clone(),
                display_or_dash(entry.timestamp.as_deref()),
                display_or_dash(entry.status.as_deref()),
            ]);
        }
    }
    rows
}

/// The API drops ids it cannot parse and reads duplicates once, so a short
/// answer is normal. It is reported as a count because the `resourceIds` that come back
/// are normalized, so the missing ones cannot be named reliably.
fn report_missing_resources(asked: usize, answered: usize, reasons: &str) {
    if answered < asked {
        eprintln!(
            "{}",
            format!("{answered} of {asked} resource(s) answered; {reasons}").yellow()
        );
    }
}

fn distinct_resources(results: &[ResourceDiffData]) -> usize {
    results
        .iter()
        .filter_map(|r| r.resource_id.as_deref())
        .collect::<std::collections::HashSet<_>>()
        .len()
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

    fn block_lines(r: &ResourceData, profile: Option<&str>) -> Vec<String> {
        resource_block(r, profile)
            .lines()
            .skip(1)
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect()
    }

    #[test]
    fn resource_block_leads_with_id_type_and_policies() {
        let r = typed_resource("Hosts", "EC2_Instances", &[("Region", "eu-west-1")]);
        assert_eq!(
            block_lines(&r, None),
            [
                "Resource ID 4013226:host_id=i-077a1626590913a16",
                "Type Hosts / EC2_Instances",
                "Policies -",
                "Region eu-west-1"
            ]
        );
    }

    #[test]
    fn resource_block_names_the_profile_only_when_asked() {
        let r = typed_resource("Hosts", "EC2_Instances", &[]);
        assert_eq!(block_lines(&r, Some("prod"))[0], "Profile prod");
        assert!(!resource_block(&r, None).contains("Profile"));
    }

    #[test]
    fn resource_block_shows_only_the_columns_the_resource_carries() {
        let host = typed_resource("Hosts", "EC2_Instances", &[("Region", "eu")]);
        let pod = typed_resource("Kubernetes", "Pods", &[("Namespace", "kube-system")]);
        let out = resource_blocks(&[("p".into(), host), ("p".into(), pod)], false);
        let (first, second) = out.split_once("\n\n").unwrap();
        assert!(first.contains("Region") && !first.contains("Namespace"));
        assert!(second.contains("Namespace") && !second.contains("Region"));
    }

    #[test]
    fn resource_block_keeps_name_as_the_heading_not_a_field() {
        let r = typed_resource("Hosts", "EC2_Instances", &[("Name", "web-01")]);
        let out = resource_block(&r, None);
        assert!(out.lines().next().unwrap().contains("web-01"));
        assert!(!block_lines(&r, None).iter().any(|l| l.starts_with("Name")));
    }

    #[test]
    fn resource_block_dashes_a_missing_classification() {
        let mut r = typed_resource("Hosts", "EC2_Instances", &[]);
        r.category = None;
        assert_eq!(block_lines(&r, None)[1], "Type - / EC2_Instances");
    }

    #[test]
    fn resource_block_puts_each_policy_on_its_own_line() {
        let mut r = typed_resource("Kubernetes", "Deployments", &[]);
        r.health_policies = vec![
            policy("Pod CPU utilization high", "healthy"),
            policy("Pod memory utilization high", "critical"),
        ];
        let lines = block_lines(&r, None);
        assert_eq!(lines[2], "Policies Pod CPU utilization high (healthy)");
        assert_eq!(lines[3], "Pod memory utilization high (critical)");
    }

    #[test]
    fn split_long_list_breaks_only_long_comma_lists() {
        assert_eq!(
            split_long_list("1 running, 2 pending"),
            ["1 running, 2 pending"]
        );
        let long =
            "app.kubernetes.io/name=shop, app.kubernetes.io/instance=shop-prod, team=checkout";
        assert_eq!(
            split_long_list(long),
            [
                "app.kubernetes.io/name=shop",
                "app.kubernetes.io/instance=shop-prod",
                "team=checkout"
            ]
        );
        let unbroken = "x".repeat(WRAP_LIST_LENGTH + 1);
        assert_eq!(split_long_list(&unbroken), std::slice::from_ref(&unbroken));
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
    fn history_table_has_one_row_per_sample_naming_its_resource() {
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

        assert_eq!(
            history_table(&results, "p"),
            vec![
                vec!["p", "id-1", "2026-07-01T00:00:00Z", "Healthy"],
                vec!["p", "id-2", "2026-07-01T00:00:00Z", "Critical"],
                vec!["p", "id-2", "2026-07-02T00:00:00Z", "Healthy"],
            ]
        );
    }

    #[test]
    fn history_table_keeps_a_resource_without_samples() {
        let results = [
            history(Some("id-1"), &[]),
            history(Some("id-2"), &[("2026-07-01T00:00:00Z", "Healthy")]),
        ];

        assert_eq!(
            history_table(&results, "p"),
            vec![
                vec!["p", "id-1", "-", "-"],
                vec!["p", "id-2", "2026-07-01T00:00:00Z", "Healthy"],
            ]
        );
    }

    #[test]
    fn history_table_dashes_a_missing_resource_id() {
        let results = [history(None, &[("2026-07-01T00:00:00Z", "Healthy")])];
        assert_eq!(history_table(&results, "p")[0][1], "-");
    }

    #[test]
    fn history_json_keeps_one_entry_per_resource() {
        let results = [
            history(Some("id-1"), &[("2026-07-01T00:00:00Z", "Healthy")]),
            history(Some("id-2"), &[]),
        ];
        let rows: Vec<Value> = results.iter().map(history_to_json).collect();

        assert_eq!(
            rows,
            vec![
                json!({
                    "resource_id": "id-1",
                    "health_history": [{ "timestamp": "2026-07-01T00:00:00Z", "status": "Healthy" }]
                }),
                json!({ "resource_id": "id-2", "health_history": [] }),
            ]
        );
    }

    #[test]
    fn history_json_keeps_a_missing_resource_id_as_null() {
        let v = history_to_json(&history(None, &[]));
        assert_eq!(v["resource_id"], Value::Null);
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
    fn distinct_resources_counts_a_resource_with_two_sources_once() {
        let mut other_source = diff("unchanged", vec![]);
        other_source.source = Some("AWS".to_string());
        let mut other_resource = diff("unchanged", vec![]);
        other_resource.resource_id = Some("7000098:a=backend".to_string());
        let mut unnamed = diff("unchanged", vec![]);
        unnamed.resource_id = None;

        let results = [
            diff("changed", vec![]),
            other_source,
            other_resource,
            unnamed,
        ];
        assert_eq!(distinct_resources(&results), 2);
    }

    #[test]
    fn path_segments_split_on_dots_and_bracket_keys() {
        assert_eq!(
            path_segments("spec.containers[app].volumeMounts[kube-api]"),
            ["spec", "containers", "[app]", "volumeMounts", "[kube-api]"]
        );
        assert_eq!(
            path_segments("metadata.labels[app.kubernetes.io/name]"),
            ["metadata", "labels", "[app.kubernetes.io/name]"]
        );
        assert_eq!(path_segments(""), ["-"]);
    }

    #[test]
    fn diff_block_groups_siblings_and_collapses_lone_chains() {
        let result = diff(
            "changed",
            vec![
                change("metadata.uid", json!("a"), json!("b")),
                change("metadata.managedFields[0].time", json!("t1"), json!("t2")),
                change("spec.replicas", json!(3), json!(10)),
            ],
        );
        let tree = diff_block(&result, false);
        let (_, tree) = tree.split_once("\n\n").unwrap();
        assert_eq!(
            tree,
            "  metadata\n\
             \x20   uid\n\
             \x20     - a\n\
             \x20     + b\n\
             \x20   managedFields[0].time\n\
             \x20     - t1\n\
             \x20     + t2\n\
             \x20 spec.replicas\n\
             \x20   - 3\n\
             \x20   + 10\n"
        );
    }

    #[test]
    fn diff_block_heads_with_the_resource_and_window() {
        let mut result = diff("changed", vec![change("spec.replicas", json!(3), json!(4))]);
        result.compared_from = Some("2026-09-24T10:22:52.644094231Z".to_string());
        result.compared_to = Some("2026-09-27T13:03:15.926855850Z".to_string());
        let out = diff_block(&result, false);
        let header: Vec<&str> = out.lines().take(5).collect();
        assert_eq!(
            header,
            [
                "7000098:a=frontend",
                "  Source    OTEL",
                "  Outcome   changed",
                "  Compared  2026-09-24T10:22:52Z → 2026-09-27T13:03:15Z",
                "  Changes   1",
            ]
        );
    }

    /// `unchanged` and `created` are answers about a resource. Dropping them
    /// would report on fewer resources than the API replied about.
    #[test]
    fn diff_block_keeps_an_outcome_that_carries_no_changes() {
        let out = diff_blocks(&[diff("unchanged", vec![])], false);
        assert_eq!(
            out,
            "7000098:a=frontend\n  Source    OTEL\n  Outcome   unchanged\n  Compared  -\n"
        );
    }

    #[test]
    fn an_added_subtree_prints_as_yaml_on_the_after_side_only() {
        let added = json!({
            "name": "token",
            "projected": { "defaultMode": 420, "sources": [{ "path": "token" }, "raw"] }
        });
        let result = diff(
            "changed",
            vec![change("spec.volumes[token]", Value::Null, added)],
        );
        let out = diff_block(&result, false);
        let (_, tree) = out.split_once("\n\n").unwrap();
        assert_eq!(
            tree,
            "  spec.volumes[token]\n\
             \x20   + name: token\n\
             \x20   + projected:\n\
             \x20   +   defaultMode: 420\n\
             \x20   +   sources:\n\
             \x20   +     - path: token\n\
             \x20   +     - raw\n"
        );
    }

    #[test]
    fn a_removed_field_prints_on_the_before_side_only() {
        let result = diff(
            "changed",
            vec![change("spec.paused", json!(true), Value::Null)],
        );
        assert!(diff_block(&result, false).ends_with("  spec.paused\n    - true\n"));
    }

    /// Strings print bare; anything YAML would read as another type is quoted, so
    /// a number is not confused with the string of that number.
    #[test]
    fn yaml_scalars_quote_only_what_would_read_as_another_type() {
        assert_eq!(yaml_scalar(&json!("shop:1.5.0")), "shop:1.5.0");
        assert_eq!(yaml_scalar(&json!(10)), "10");
        assert_eq!(yaml_scalar(&json!("10")), r#""10""#);
        assert_eq!(yaml_scalar(&json!(true)), "true");
        assert_eq!(yaml_scalar(&json!("true")), r#""true""#);
        assert_eq!(yaml_scalar(&json!("")), r#""""#);
        assert_eq!(yaml_scalar(&json!("a: b")), r#""a: b""#);
        assert_eq!(yaml_scalar(&json!({})), "{}");
        assert_eq!(yaml_scalar(&json!([])), "[]");
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
            health_policies: Vec::new(),
        };

        assert_eq!(
            resource_to_json(&item, false, "prod"),
            json!({
                "resource_id": "4013226:host_id=i-077a",
                "name": "prod-api-01",
                "category": "Hosts",
                "type": "EC2_Instances",
                "health_policies": [],
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
            health_policies: Vec::new(),
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
