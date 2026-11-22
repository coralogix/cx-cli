pub mod api;

use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::Value;
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use crate::time::parse_timestamp_epoch_millis;
use api::{
    display_enum, Application, Event, FunnelMatchedUser, HeatmapElement, Page, ProductAnalyticsApi,
    QueryFunnelConversionResponse, QueryHeatmapTopElementsResponse, QueryInput, QueryKind,
    QueryPathwayPathsResponse, QueryRumEventResponse, SavedObject, SavedObjectKind, Settings,
    ViewSessions,
};

#[derive(clap::Args, Clone, Debug, Default)]
pub struct TimeRangeArgs {
    #[arg(
        long,
        help = "Start of the time range: `now`, `now-1h`, `now - 3d`, or ISO-8601. Default: now-1h, or the saved range when running a saved object"
    )]
    pub start: Option<String>,

    #[arg(
        long,
        help = "End of the time range: `now`, `now-1h`, `now - 3d`, or ISO-8601. Default: now"
    )]
    pub end: Option<String>,
}

impl TimeRangeArgs {
    // Fixed range in epoch milliseconds, or None to keep a saved object's own range.
    pub fn resolve(&self, runs_saved_object: bool) -> Result<Option<(i64, i64)>> {
        if runs_saved_object && self.start.is_none() && self.end.is_none() {
            return Ok(None);
        }
        let start = self.start.as_deref().unwrap_or("now-1h");
        let end = self.end.as_deref().unwrap_or("now");
        let start_millis = parse_timestamp_epoch_millis(start)?;
        let end_millis = parse_timestamp_epoch_millis(end)?;
        if end_millis <= start_millis {
            bail!("--end ({end}) must be strictly after --start ({start})");
        }
        Ok(Some((start_millis, end_millis)))
    }
}

#[derive(clap::Args, Clone, Debug, Default)]
pub struct PageArgs {
    #[arg(
        long,
        help = "Maximum items to return (server default 50, or 20 for saved lists; capped at 500)"
    )]
    pub page_size: Option<u32>,

    #[arg(
        long,
        help = "The nextPageToken printed by the previous page. Valid for 15 minutes"
    )]
    pub page_token: Option<String>,
}

type RowRenderer = fn(&Value) -> Result<Vec<String>>;

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading JSON from stdin...".dimmed());
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        buf
    } else {
        eprintln!("{}", format!("Reading JSON from {path}...").dimmed());
        std::fs::read_to_string(path)
            .map_err(|error| anyhow::anyhow!("could not read {path}: {error}"))?
    };
    serde_json::from_str(&raw).map_err(|error| anyhow::anyhow!("invalid JSON in {path}: {error}"))
}

fn profile_prefix(include_profile: bool, profile: &str) -> String {
    if include_profile {
        format!("profile '{profile}': ")
    } else {
        String::new()
    }
}

fn warn_if_costly(costly: bool, include_profile: bool, profile: &str) {
    if costly {
        eprintln!(
            "{}",
            format!(
                "warning: {}the time range is over 90 days, so this query was costly.",
                profile_prefix(include_profile, profile)
            )
            .yellow()
        );
    }
}

fn format_pct(value: Option<f64>) -> String {
    value
        .map(|pct| format!("{pct:.1}%"))
        .unwrap_or_else(|| "-".to_string())
}

fn or_dash(value: Option<&str>) -> String {
    value.unwrap_or("-").to_string()
}

fn application_row(item: &Value) -> Result<Vec<String>> {
    let app: Application = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(app.app_name.as_deref()),
        or_dash(app.sessions.as_deref()),
        or_dash(app.crash_free_sessions.as_deref()),
        or_dash(app.users.as_deref()),
        or_dash(app.crash_free_users.as_deref()),
    ])
}

fn view_row(item: &Value) -> Result<Vec<String>> {
    let view: ViewSessions = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(view.view.as_deref()),
        or_dash(view.sessions.as_deref()),
    ])
}

fn event_row(item: &Value) -> Result<Vec<String>> {
    let event: Event = serde_json::from_value(item.clone())?;
    let event_ref = event.event.as_ref();
    Ok(vec![
        or_dash(event.event_id.as_deref()),
        display_enum(event_ref.and_then(|e| e.source.as_deref())),
        or_dash(event_ref.and_then(|e| e.name.as_deref())),
        or_dash(event.count.as_deref()),
        format_pct(event.dead_clicks_pct),
        format_pct(event.rage_clicks_pct),
        or_dash(event.session_count.as_deref()),
    ])
}

fn matched_user_row(item: &Value) -> Result<Vec<String>> {
    let user: FunnelMatchedUser = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(user.user_id.as_deref()),
        or_dash(user.example_session_id.as_deref()),
        render::bool_display(user.has_session_replay),
        or_dash(user.platform.as_deref()),
        or_dash(user.os.as_deref()),
        or_dash(user.country.as_deref()),
    ])
}

fn saved_object_row(item: &Value) -> Result<Vec<String>> {
    let saved: SavedObject = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(saved.id.as_deref()),
        or_dash(saved.name.as_deref()),
        or_dash(saved.app_name.as_deref()),
        saved.details(),
        or_dash(saved.updated_at.as_deref()),
        saved
            .version
            .map(|version| version.to_string())
            .unwrap_or_else(|| "-".to_string()),
    ])
}

const APPLICATION_HEADERS: &[&str] = &[
    "App",
    "Sessions",
    "Crash-free sessions",
    "Users",
    "Crash-free users",
];
const VIEW_HEADERS: &[&str] = &["View", "Sessions"];
const EVENT_HEADERS: &[&str] = &[
    "Event ID", "Source", "Name", "Clicks", "Dead", "Rage", "Sessions",
];
const MATCHED_USER_HEADERS: &[&str] = &[
    "User",
    "Example session",
    "Replay",
    "Platform",
    "OS",
    "Country",
];
const SAVED_OBJECT_HEADERS: &[&str] = &["ID", "Name", "App", "Details", "Updated", "Version"];

fn render_pages(
    pages: Vec<(String, Page)>,
    include_profile: bool,
    output: OutputFormat,
    headers: &[&str],
    row: RowRenderer,
    empty_message: &str,
) -> Result<()> {
    let mut all_json: Vec<Value> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (profile, page) in pages {
        warn_if_costly(page.costly, include_profile, &profile);
        if let Some(token) = &page.next_page_token {
            let total = page
                .total_size
                .as_deref()
                .map(|total| format!(" ({total} in total)"))
                .unwrap_or_default();
            eprintln!(
                "{}",
                format!(
                    "{}more results{total}: pass --page-token {token}",
                    profile_prefix(include_profile, &profile)
                )
                .dimmed()
            );
        }
        for mut item in page.items {
            if matches!(output, OutputFormat::Text) {
                let mut cells = vec![profile.clone()];
                cells.extend(row(&item)?);
                rows.push(cells);
            }
            if include_profile {
                if let Value::Object(ref mut map) = item {
                    map.insert("profile".to_string(), Value::String(profile.clone()));
                }
            }
            all_json.push(item);
        }
    }

    match output {
        OutputFormat::Json => render::render_json(&all_json)?,
        OutputFormat::Toon => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if rows.is_empty() {
                render::print_no_results(empty_message);
                return Ok(());
            }
            render::render_table(headers, rows, include_profile);
        }
    }
    Ok(())
}

fn render_responses(
    responses: Vec<(String, Value)>,
    include_profile: bool,
    output: OutputFormat,
    text: fn(&Value) -> Result<()>,
) -> Result<()> {
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut response) in responses {
        warn_if_costly(api::is_costly(&response), include_profile, &profile);
        if matches!(output, OutputFormat::Text) {
            if include_profile {
                println!("{}", format!("[{profile}]").dimmed());
            }
            text(&response)?;
            continue;
        }
        if include_profile {
            render::tag_get_result(&mut response, &profile);
        }
        all_results.push(response);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }
    Ok(())
}

fn print_application(response: &Value) -> Result<()> {
    let app: Application = serde_json::from_value(response.clone())?;
    println!("{}: {}", "App".bold(), or_dash(app.app_name.as_deref()));
    println!(
        "{}: {} ({} crash-free)",
        "Sessions (30d)".bold(),
        or_dash(app.sessions.as_deref()),
        or_dash(app.crash_free_sessions.as_deref())
    );
    println!(
        "{}: {} ({} crash-free)",
        "Users (30d)".bold(),
        or_dash(app.users.as_deref()),
        or_dash(app.crash_free_users.as_deref())
    );
    Ok(())
}

fn print_saved_object(response: &Value) -> Result<()> {
    let saved: SavedObject = serde_json::from_value(response.clone())?;
    println!("{}: {}", "Name".bold(), or_dash(saved.name.as_deref()));
    println!("{}: {}", "ID".bold(), or_dash(saved.id.as_deref()));
    println!("{}: {}", "App".bold(), or_dash(saved.app_name.as_deref()));
    println!("{}: {}", "Details".bold(), saved.details());
    println!("{}: {}", "Owner".bold(), or_dash(saved.owner.as_deref()));
    println!(
        "{}: {}",
        "Updated".bold(),
        or_dash(saved.updated_at.as_deref())
    );
    println!();
    println!("{}", serde_json::to_string_pretty(response)?);
    Ok(())
}

fn print_event(response: &Value) -> Result<()> {
    let parsed: QueryRumEventResponse = serde_json::from_value(response.clone())?;
    let Some(event) = parsed.event else {
        render::print_no_results("Event not found.");
        return Ok(());
    };
    let event_ref = event.event.as_ref();
    println!(
        "{}: {}",
        "Event ID".bold(),
        or_dash(event.event_id.as_deref())
    );
    println!(
        "{}: {}",
        "Source".bold(),
        display_enum(event_ref.and_then(|e| e.source.as_deref()))
    );
    println!(
        "{}: {}",
        "Name".bold(),
        or_dash(event_ref.and_then(|e| e.name.as_deref()))
    );
    if let Some(count) = &event.count {
        println!("{}: {count}", "Clicks".bold());
        println!(
            "{}: {} ({})",
            "Dead clicks".bold(),
            or_dash(event.dead_clicks.as_deref()),
            format_pct(event.dead_clicks_pct)
        );
        println!(
            "{}: {} ({})",
            "Rage clicks".bold(),
            or_dash(event.rage_clicks.as_deref()),
            format_pct(event.rage_clicks_pct)
        );
    }
    if let Some(sessions) = &event.session_count {
        println!("{}: {sessions}", "Sessions".bold());
    }
    Ok(())
}

fn print_funnel_conversion(response: &Value) -> Result<()> {
    let parsed: QueryFunnelConversionResponse = serde_json::from_value(response.clone())?;
    let Some(conversion) = parsed.conversion else {
        render::print_no_results("No funnel result.");
        return Ok(());
    };
    println!(
        "{}: {}  {}: {}",
        "App".bold(),
        or_dash(conversion.app_name.as_deref()),
        "Mode".bold(),
        display_enum(conversion.mode.as_deref())
    );
    if let Some(summary) = &conversion.summary {
        println!(
            "{}: {} entered, {} converted ({})",
            "Summary".bold(),
            or_dash(summary.entered_count.as_deref()),
            or_dash(summary.converted_count.as_deref()),
            format_pct(summary.converted_pct)
        );
        if let Some(drop_off) = &summary.biggest_drop_off {
            println!(
                "{}: {} -> {} ({} of previous)",
                "Biggest drop-off".bold(),
                or_dash(drop_off.from_step_id.as_deref()),
                or_dash(drop_off.to_step_id.as_deref()),
                format_pct(drop_off.dropped_pct_of_previous)
            );
        }
    }
    let groups: Vec<(String, &Vec<api::FunnelStepResult>)> = if conversion.groups.is_empty() {
        vec![(String::new(), &conversion.steps)]
    } else {
        conversion
            .groups
            .iter()
            .map(|group| (or_dash(group.group.as_deref()), &group.steps))
            .collect()
    };
    let with_groups = !conversion.groups.is_empty();
    let rows: Vec<Vec<String>> = groups
        .iter()
        .flat_map(|(group, steps)| {
            steps.iter().map(move |step| {
                let mut row = vec![String::new()];
                if with_groups {
                    row.push(group.clone());
                }
                row.extend([
                    or_dash(step.step_id.as_deref()),
                    or_dash(step.label.as_deref()),
                    or_dash(step.count.as_deref()),
                    format_pct(step.converted_pct_of_total),
                    format_pct(step.converted_pct_of_previous),
                    or_dash(step.dropped_count.as_deref()),
                    or_dash(step.avg_time_from_previous.as_deref()),
                ]);
                row
            })
        })
        .collect();
    let headers = [
        "Group",
        "Step",
        "Label",
        "Count",
        "Of first",
        "Of previous",
        "Dropped",
        "Avg time",
    ];
    let headers = if with_groups {
        &headers[..]
    } else {
        &headers[1..]
    };
    render::render_table(headers, rows, false);
    Ok(())
}

fn print_pathway_paths(response: &Value) -> Result<()> {
    let parsed: QueryPathwayPathsResponse = serde_json::from_value(response.clone())?;
    let Some(paths) = parsed.pathway_paths else {
        render::print_no_results("No pathway result.");
        return Ok(());
    };
    println!(
        "{}: {}  {}: {}  {}: {}",
        "App".bold(),
        or_dash(paths.app_name.as_deref()),
        "View".bold(),
        or_dash(paths.view.as_deref()),
        "Direction".bold(),
        display_enum(paths.direction.as_deref())
    );
    if paths.paths.is_empty() {
        render::print_no_results("No paths found.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = paths
        .paths
        .iter()
        .map(|path| {
            vec![
                String::new(),
                or_dash(path.count.as_deref()),
                path.pages.join(" -> "),
            ]
        })
        .collect();
    render::render_table(&["Sessions", "Path"], rows, false);
    Ok(())
}

fn print_heatmap_elements(response: &Value) -> Result<()> {
    let parsed: QueryHeatmapTopElementsResponse = serde_json::from_value(response.clone())?;
    let Some(data) = parsed.heatmap_data else {
        render::print_no_results("No heatmap result.");
        return Ok(());
    };
    println!(
        "{}: {}  {}: {}  {}: {}",
        "App".bold(),
        or_dash(data.app_name.as_deref()),
        "View".bold(),
        or_dash(data.view.as_deref()),
        "Clicks".bold(),
        display_enum(data.action_name_filter.as_deref())
    );
    if data.elements.is_empty() {
        render::print_no_results("No clicked elements found.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = data
        .elements
        .iter()
        .map(|element: &HeatmapElement| {
            vec![
                String::new(),
                or_dash(element.event_id.as_deref()),
                or_dash(element.name.as_deref()),
                or_dash(element.clicks.as_deref()),
                format_pct(element.dead_clicks_pct),
                format_pct(element.rage_clicks_pct),
            ]
        })
        .collect();
    render::render_table(&["Event ID", "Name", "Clicks", "Dead", "Rage"], rows, false);
    Ok(())
}

fn print_settings(response: &Value) -> Result<()> {
    let settings: Settings = serde_json::from_value(response.clone())?;
    println!(
        "{}: {}",
        "Product Analytics enabled".bold(),
        render::bool_display(Some(settings.enabled))
    );
    Ok(())
}

fn read_optional_file(from_file: Option<&str>) -> Result<Option<Value>> {
    from_file.map(read_from_file).transpose()
}

pub async fn run_applications_list(
    targets: &[Arc<ExecutionTarget>],
    app_names: &[String],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching Product Analytics applications...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        Ok(api.list_applications(app_names).await?)
    })
    .await;
    render_pages(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
        output,
        APPLICATION_HEADERS,
        application_row,
        "No applications with sessions in the last 30 days.",
    )
}

pub async fn run_applications_get(
    targets: &[Arc<ExecutionTarget>],
    app_name: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching application {app_name}...").dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        Ok(api.get_application(app_name).await?)
    })
    .await;
    render_responses(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
        output,
        print_application,
    )
}

pub async fn run_list_query(
    targets: &[Arc<ExecutionTarget>],
    kind: QueryKind,
    from_file: Option<&str>,
    input: &QueryInput,
    output: OutputFormat,
) -> Result<()> {
    let (headers, row, empty_message): (&[&str], RowRenderer, &str) = match kind {
        QueryKind::Views => (VIEW_HEADERS, view_row, "No views found."),
        QueryKind::Events => (EVENT_HEADERS, event_row, "No events found."),
        QueryKind::FunnelMatches => (MATCHED_USER_HEADERS, matched_user_row, "No matches found."),
        _ => unreachable!("{kind:?} is not a paged query"),
    };
    let body = api::build_query_body(kind, read_optional_file(from_file)?, input)?;
    eprintln!("{}", "Running Product Analytics query...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ProductAnalyticsApi::new(&t.client);
            Ok(api.query_page(kind, &body).await?)
        }
    })
    .await;
    render_pages(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
        output,
        headers,
        row,
        empty_message,
    )
}

pub async fn run_query(
    targets: &[Arc<ExecutionTarget>],
    kind: QueryKind,
    from_file: Option<&str>,
    input: &QueryInput,
    output: OutputFormat,
) -> Result<()> {
    let text: fn(&Value) -> Result<()> = match kind {
        QueryKind::FunnelConversion => print_funnel_conversion,
        QueryKind::PathwayPaths => print_pathway_paths,
        QueryKind::HeatmapTopElements => print_heatmap_elements,
        _ => unreachable!("{kind:?} is not a single-result query"),
    };
    let body = api::build_query_body(kind, read_optional_file(from_file)?, input)?;
    eprintln!("{}", "Running Product Analytics query...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ProductAnalyticsApi::new(&t.client);
            Ok(api.query(kind, &body).await?)
        }
    })
    .await;
    render_responses(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
        output,
        text,
    )
}

pub async fn run_events_get(
    targets: &[Arc<ExecutionTarget>],
    event_id: &str,
    from_file: Option<&str>,
    input: &QueryInput,
    output: OutputFormat,
) -> Result<()> {
    let body = api::build_query_body(QueryKind::Event, read_optional_file(from_file)?, input)?;
    eprintln!("{}", format!("Fetching event {event_id}...").dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ProductAnalyticsApi::new(&t.client);
            Ok(api.query_event(event_id, &body).await?)
        }
    })
    .await;
    render_responses(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
        output,
        print_event,
    )
}

pub async fn run_saved_list(
    targets: &[Arc<ExecutionTarget>],
    kind: SavedObjectKind,
    app_names: &[String],
    page: &PageArgs,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching saved {}s...", kind.label()).dimmed()
    );
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        Ok(api
            .list_saved(kind, app_names, page.page_size, page.page_token.as_deref())
            .await?)
    })
    .await;
    render_pages(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
        output,
        SAVED_OBJECT_HEADERS,
        saved_object_row,
        &format!("No saved {}s found.", kind.label()),
    )
}

pub async fn run_saved_get(
    targets: &[Arc<ExecutionTarget>],
    kind: SavedObjectKind,
    id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching {} {id}...", kind.label()).dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        Ok(api.get_saved(kind, id).await?)
    })
    .await;
    render_responses(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
        output,
        print_saved_object,
    )
}

pub async fn run_saved_write(
    targets: &[Arc<ExecutionTarget>],
    kind: SavedObjectKind,
    from_file: &str,
    update: Option<(&str, Option<&str>)>,
    output: OutputFormat,
) -> Result<()> {
    let file_body = read_from_file(from_file)?;
    let (body, update_mask) = match update {
        Some((_, mask)) => {
            let (body, mask) = api::prepare_update(file_body, mask)?;
            (body, Some(mask))
        }
        None => (file_body, None),
    };
    let (progress, verb) = if update.is_some() {
        ("Updating", "Updated")
    } else {
        ("Creating", "Created")
    };
    eprintln!("{}", format!("{progress} {}...", kind.label()).dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        let update_mask = update_mask.clone();
        async move {
            let api = ProductAnalyticsApi::new(&t.client);
            let saved = match (update, update_mask) {
                (Some((id, _)), Some(mask)) => api.update_saved(kind, id, &body, &mask).await?,
                _ => api.create_saved(kind, &body).await?,
            };
            Ok(saved)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut saved) in report_errors_and_collect_successes(per_profile)? {
        let parsed: SavedObject = serde_json::from_value(saved.clone())?;
        render::print_created(
            verb,
            kind.label(),
            parsed.name.as_deref(),
            parsed.id.as_deref(),
            &profile,
        );
        if include_profile {
            render::tag_get_result(&mut saved, &profile);
        }
        all_results.push(saved);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }
    Ok(())
}

pub async fn run_saved_delete(
    targets: &[Arc<ExecutionTarget>],
    kind: SavedObjectKind,
    id: &str,
) -> Result<()> {
    eprintln!("{}", format!("Deleting {} {id}...", kind.label()).dimmed());
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        api.delete_saved(kind, id).await?;
        Ok(())
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("{} {id} deleted in profile '{profile}'.", kind.label()).green()
        );
    }
    Ok(())
}

pub async fn run_settings_get(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching Product Analytics settings...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        Ok(api.get_settings().await?)
    })
    .await;
    render_responses(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
        output,
        print_settings,
    )
}

pub async fn run_settings_update(
    targets: &[Arc<ExecutionTarget>],
    enabled: bool,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Updating Product Analytics settings...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        Ok(api.update_settings(enabled).await?)
    })
    .await;
    render_responses(
        report_errors_and_collect_successes(per_profile)?,
        include_profile,
        output,
        print_settings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time_range(start: Option<&str>, end: Option<&str>) -> TimeRangeArgs {
        TimeRangeArgs {
            start: start.map(String::from),
            end: end.map(String::from),
        }
    }

    #[test]
    fn start_and_end_become_fixed_millis() {
        let args = time_range(Some("2026-11-16T00:00:00Z"), Some("2026-11-17T00:00:00Z"));
        assert_eq!(
            args.resolve(false).unwrap(),
            Some((1_794_787_200_000, 1_794_873_600_000))
        );
    }

    #[test]
    fn subsecond_start_and_end_keep_their_milliseconds() {
        let args = time_range(
            Some("2026-11-16T00:00:00.100Z"),
            Some("2026-11-16T00:00:00.900Z"),
        );
        assert_eq!(
            args.resolve(false).unwrap(),
            Some((1_794_787_200_100, 1_794_787_200_900))
        );
    }

    #[test]
    fn no_flags_default_to_last_hour_unless_running_a_saved_object() {
        let (start, end) = time_range(None, None).resolve(false).unwrap().unwrap();
        assert!((3_600_000..=3_601_000).contains(&(end - start)));
        assert_eq!(time_range(None, None).resolve(true).unwrap(), None);
        assert!(time_range(Some("now-1d"), None)
            .resolve(true)
            .unwrap()
            .is_some());
    }

    #[test]
    fn reversed_range_is_rejected() {
        assert!(time_range(Some("now-1h"), Some("now-2h"))
            .resolve(false)
            .is_err());
    }

    #[test]
    fn saved_object_row_has_one_cell_per_header() {
        let row = saved_object_row(&serde_json::json!({"id": "1", "name": "n"})).unwrap();
        assert_eq!(row.len(), SAVED_OBJECT_HEADERS.len());
    }
}
