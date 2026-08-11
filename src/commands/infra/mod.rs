use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

pub mod api;

use api::{HealthHistoryEntry, InfraApi, ListResourcesParams, ResourceData, ResourceTypeMapping};

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;

/// JSON key for the source profile when merging multi-profile infra REST rows.
const JSON_KEY_PROFILE: &str = "profile";

/// Scope filter keys accepted by the infrastructure resources API. Validated
/// client-side so a typo fails fast instead of round-tripping for a 400.
const ALLOWED_SCOPE_KEYS: [&str; 3] = ["service", "environment", "team"];

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

/// `cx infra resources list` - list resources of a given category and type.
#[allow(clippy::too_many_arguments)]
pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    category: &str,
    resource_type: &str,
    name_filter: Option<&str>,
    scope: &[String],
    start_row: Option<i64>,
    end_row: Option<i64>,
    output: OutputFormat,
) -> Result<()> {
    let category = require_non_empty(category, "--category")?;
    let resource_type = require_non_empty(resource_type, "--type")?;
    let name_filter = name_filter.map(str::trim).filter(|s| !s.is_empty());
    let scope_filters = parse_scope_filters(scope)?;
    validate_page_window(start_row, end_row)?;

    eprintln!("{}", "Fetching resources...".dimmed());

    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |target| {
        let scope_filters = scope_filters.clone();
        let category = category.to_string();
        let resource_type = resource_type.to_string();
        let name_filter = name_filter.map(String::from);
        async move {
            let api = InfraApi::new(&target.client);
            let params = ListResourcesParams {
                category: &category,
                resource_type: &resource_type,
                name_filter: name_filter.as_deref(),
                scope_filters: &scope_filters,
                start_row,
                end_row,
            };
            Ok(api.list(&params).await?)
        }
    })
    .await;

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
            let rows: Vec<Vec<String>> = merged
                .iter()
                .map(|(profile, r)| {
                    vec![
                        profile.clone(),
                        display_or_dash(r.resource_id.as_deref()),
                        display_or_dash(r.name.as_deref()),
                    ]
                })
                .collect();
            render::render_table(&["Resource ID", "Name"], rows, include_profile);
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
    resource_id: &str,
    output: OutputFormat,
) -> Result<()> {
    let resource_id = require_non_empty(resource_id, "resource id")?;
    let target = single_target(targets, "health-history")?;

    eprintln!(
        "{}",
        format!("Fetching health history for '{resource_id}'...").dimmed()
    );

    let resp = InfraApi::new(&target.client)
        .health_history(resource_id)
        .await
        .with_context(|| format!("profile '{}' failed", target.profile_name))?;
    let history = resp.health_history;

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows: Vec<Value> = history.iter().map(health_entry_to_json).collect();
            render_machine_rows(output, &rows)?;
        }
        OutputFormat::Text => {
            if history.is_empty() {
                render::print_no_results("No health history found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = history
                .iter()
                .map(|entry| {
                    vec![
                        target.profile_name.clone(),
                        display_or_dash(entry.timestamp.as_deref()),
                        display_or_dash(entry.status.as_deref()),
                    ]
                })
                .collect();
            render::render_table(&["Timestamp", "Status"], rows, false);
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
    output: OutputFormat,
) -> Result<()> {
    let resource_id = require_non_empty(resource_id, "resource id")?;
    let target = single_target(targets, "raw-data")?;

    eprintln!(
        "{}",
        format!("Fetching raw resource data for '{resource_id}'...").dimmed()
    );

    let resp = InfraApi::new(&target.client)
        .raw_data(resource_id)
        .await
        .with_context(|| format!("profile '{}' failed", target.profile_name))?;

    // A 200 with null raw data means the document is cleanly missing, so note it
    // on stderr and render an empty result rather than failing.
    let results: Vec<Value> = match resp.raw_data {
        Some(doc) => vec![doc],
        None => {
            eprintln!("{}", "no raw data for this resource".yellow());
            Vec::new()
        }
    };

    match output {
        OutputFormat::Json => render::render_json_auto(&results)?,
        OutputFormat::Toon => render::render_toon(&results)?,
        OutputFormat::Text => {
            render::render_get_text(&results, false, "No raw data found.", None)?;
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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
fn validate_page_window(start_row: Option<i64>, end_row: Option<i64>) -> Result<()> {
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

/// Parses repeatable `--scope key=value` flags and validates keys against
/// [`ALLOWED_SCOPE_KEYS`].
///
/// Distinct keys are combined by the API with AND ("when more than one field is
/// set, a resource must match all of them"). A key given twice is rejected: each
/// scope field holds a single value server-side, so repeating one cannot express
/// "either value".
/// Failing here makes that intent explicit instead of quietly answering a
/// different question.
fn parse_scope_filters(scope: &[String]) -> Result<Vec<(String, String)>> {
    let mut filters: Vec<(String, String)> = Vec::new();

    for raw in scope {
        let Some((key, value)) = raw.split_once('=') else {
            bail!("invalid --scope '{raw}': expected key=value");
        };
        let key = key.trim();
        let value = value.trim();
        if !ALLOWED_SCOPE_KEYS.contains(&key) {
            bail!(
                "unknown --scope key '{key}'; allowed keys: {}",
                ALLOWED_SCOPE_KEYS.join(", ")
            );
        }
        if value.is_empty() {
            bail!("invalid --scope '{raw}': value must not be empty");
        }
        if let Some((_, existing)) = filters.iter().find(|(k, _)| k == key) {
            bail!(
                "--scope key '{key}' given more than once ('{existing}' then '{value}'); \
                 each scope key accepts a single value and different keys combine with AND, \
                 so repeating one cannot match either value - run one query per value"
            );
        }
        filters.push((key.to_string(), value.to_string()));
    }

    Ok(filters)
}

/// Builds one resource row as JSON for `json` / `toon` output after fan-out.
fn resource_to_json(item: &ResourceData, include_profile: bool, profile: &str) -> Value {
    let v = json!({
        "resource_id": item.resource_id,
        "name": item.name,
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

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_scope_filters_accepts_allowed_keys() {
        let scope = vec![
            "service=checkout".to_string(),
            "environment=prod".to_string(),
            "team=platform".to_string(),
        ];
        let filters = parse_scope_filters(&scope).unwrap();
        assert_eq!(
            filters,
            vec![
                ("service".to_string(), "checkout".to_string()),
                ("environment".to_string(), "prod".to_string()),
                ("team".to_string(), "platform".to_string()),
            ]
        );
    }

    #[test]
    fn parse_scope_filters_trims_whitespace() {
        let scope = vec![" service = checkout ".to_string()];
        let filters = parse_scope_filters(&scope).unwrap();
        assert_eq!(
            filters,
            vec![("service".to_string(), "checkout".to_string())]
        );
    }

    #[test]
    fn parse_scope_filters_rejects_missing_equals() {
        let err = parse_scope_filters(&["service".to_string()]).unwrap_err();
        assert!(err.to_string().contains("expected key=value"));
    }

    #[test]
    fn parse_scope_filters_rejects_unknown_key() {
        let err = parse_scope_filters(&["region=us-east-1".to_string()]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown --scope key 'region'"));
        assert!(msg.contains("service, environment, team"));
    }

    #[test]
    fn parse_scope_filters_rejects_empty_value() {
        let err = parse_scope_filters(&["service=".to_string()]).unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn parse_scope_filters_empty_input_yields_no_filters() {
        assert!(parse_scope_filters(&[]).unwrap().is_empty());
    }

    /// Each scope field holds one value server-side and distinct keys AND
    /// together, so a repeated key cannot mean "either". The service collapses
    /// the query string into a `HashMap`, silently keeping only the last value -
    /// so this must fail here rather than quietly filter on `b` alone.
    #[test]
    fn parse_scope_filters_rejects_a_repeated_key() {
        let err =
            parse_scope_filters(&["service=a".to_string(), "service=b".to_string()]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'service' given more than once"), "got: {msg}");
        assert!(msg.contains('a') && msg.contains('b'), "got: {msg}");
    }

    /// Rejected uniformly - "at most once per key" is a simpler rule to rely on
    /// than one that quietly tolerates exact repeats.
    #[test]
    fn parse_scope_filters_rejects_a_repeated_key_even_with_the_same_value() {
        let err =
            parse_scope_filters(&["service=a".to_string(), "service=a".to_string()]).unwrap_err();
        assert!(err.to_string().contains("given more than once"));
    }

    #[test]
    fn parse_scope_filters_detects_a_repeat_after_trimming() {
        let err = parse_scope_filters(&[" service = a ".to_string(), "service=b".to_string()])
            .unwrap_err();
        assert!(err.to_string().contains("given more than once"));
    }

    // ── validate_page_window ─────────────────────────────────────────────────

    #[test]
    fn page_window_accepts_an_ascending_window_and_open_ends() {
        assert!(validate_page_window(None, None).is_ok());
        assert!(validate_page_window(Some(0), Some(100)).is_ok());
        assert!(validate_page_window(Some(100), Some(200)).is_ok());
        assert!(validate_page_window(Some(100), None).is_ok());
        assert!(validate_page_window(None, Some(50)).is_ok());
        assert!(validate_page_window(Some(0), Some(1)).is_ok());
    }

    /// The API clamps a negative `startRow` to 0 and returns 200, so without this
    /// check the caller silently gets the first window instead of an error.
    #[test]
    fn page_window_rejects_a_negative_start_row() {
        let err = validate_page_window(Some(-5), None).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--start-row must not be negative"),
            "got: {msg}"
        );
        assert!(msg.contains("-5"), "got: {msg}");
    }

    #[test]
    fn page_window_rejects_a_negative_end_row() {
        let err = validate_page_window(None, Some(-1)).unwrap_err();
        assert!(err.to_string().contains("--end-row must not be negative"));
    }

    /// An inverted window yields a row count of 0 server-side, so the caller sees
    /// an empty result set and can easily read it as "no such resources".
    #[test]
    fn page_window_rejects_an_inverted_window() {
        let err = validate_page_window(Some(200), Some(100)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("must be greater than"), "got: {msg}");
        assert!(msg.contains("100") && msg.contains("200"), "got: {msg}");
    }

    /// `--end-row` is exclusive, so an equal start and end can only ever return
    /// nothing - always a mistake rather than a meaningful request.
    #[test]
    fn page_window_rejects_an_empty_window() {
        let err = validate_page_window(Some(100), Some(100)).unwrap_err();
        assert!(err.to_string().contains("selects no rows"));
    }

    /// The repeat check must not reject distinct keys that share a value.
    #[test]
    fn parse_scope_filters_allows_distinct_keys_sharing_a_value() {
        let filters =
            parse_scope_filters(&["service=core".to_string(), "team=core".to_string()]).unwrap();
        assert_eq!(
            filters,
            vec![
                ("service".to_string(), "core".to_string()),
                ("team".to_string(), "core".to_string()),
            ]
        );
    }

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
