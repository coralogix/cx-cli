pub mod api;

use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use colored::Colorize;
use serde_json::{json, Value};

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use crate::time::parse_timestamp_epoch_seconds;
use api::{
    ActionResult, Application, ClickResult, FilterValue, FunnelResponse, MatchingUser,
    PathwayResult, ProductAnalyticsApi, QueryInput, QueryKind, SavedObject, SavedObjectKind,
    ScrollMapResponse, Settings, ViewResult,
};

#[derive(clap::Args, Clone, Debug, Default)]
pub struct TimeRangeArgs {
    #[arg(
        long,
        help = "Start of the time range: `now`, `now-1h`, `now - 3d`, or ISO-8601. Default: now-1h, or the start of the saved range when running a saved ID"
    )]
    pub start: Option<String>,

    #[arg(
        long,
        help = "End of the time range: `now`, `now-1h`, `now - 3d`, or ISO-8601. Default: now, or the end of the saved range when running a saved ID"
    )]
    pub end: Option<String>,
}

impl TimeRangeArgs {
    pub fn resolve(&self, runs_saved_object: bool) -> Result<(Option<i64>, Option<i64>)> {
        if runs_saved_object {
            let start = self
                .start
                .as_deref()
                .map(parse_timestamp_epoch_seconds)
                .transpose()?;
            let end = self
                .end
                .as_deref()
                .map(parse_timestamp_epoch_seconds)
                .transpose()?;
            return Ok((start, end));
        }
        let start = self.start.as_deref().unwrap_or("now-1h");
        let end = self.end.as_deref().unwrap_or("now");
        let start_seconds = parse_timestamp_epoch_seconds(start)?;
        let end_seconds = parse_timestamp_epoch_seconds(end)?;
        if end_seconds <= start_seconds {
            bail!("--end ({end}) must be strictly after --start ({start})");
        }
        Ok((Some(start_seconds), Some(end_seconds)))
    }
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

fn parse_id(id: &str) -> Result<i64> {
    match id.parse::<i64>() {
        Ok(parsed) if parsed > 0 => Ok(parsed),
        _ => bail!("ID must be a positive decimal number, got '{id}'"),
    }
}

pub fn require_single_target(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    if targets.len() > 1 {
        bail!(
            "saved ID {id} is scoped to one team, but {} profiles were given; the same ID can \
             name a different object in each team. Re-run once per profile with a single -p.",
            targets.len()
        );
    }
    Ok(())
}

fn format_local_time(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|time| time.with_timezone(&chrono::Local).to_rfc3339())
        .unwrap_or_else(|| seconds.to_string())
}

fn or_dash(value: Option<&str>) -> String {
    value.unwrap_or("-").to_string()
}

fn count(value: Option<&str>) -> String {
    value.unwrap_or("0").to_string()
}

fn render_machine(output: OutputFormat, results: &[Value]) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json_auto(results),
        OutputFormat::Toon => render::render_toon(results),
        OutputFormat::Text => Ok(()),
    }
}

fn application_row(item: &Value) -> Result<Vec<String>> {
    let app: Application = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(app.app_name.as_deref()),
        or_dash(app.platform.as_deref()),
        count(app.session_count.as_deref()),
        count(app.crash_free_session_count.as_deref()),
        count(app.total_users.as_deref()),
        count(app.crash_free_user_count.as_deref()),
    ])
}

fn view_row(item: &Value) -> Result<Vec<String>> {
    let view: ViewResult = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(view.view_name.as_deref()),
        count(view.session_count.as_deref()),
    ])
}

fn action_row(item: &Value) -> Result<Vec<String>> {
    let action: ActionResult = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(action.event_type.as_deref()),
        or_dash(action.event_target.as_deref()),
        count(action.session_count.as_deref()),
    ])
}

fn filter_value_row(item: &Value) -> Result<Vec<String>> {
    let value: FilterValue = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(value.value.as_deref()),
        count(value.count.as_deref()),
    ])
}

fn matching_user_row(item: &Value) -> Result<Vec<String>> {
    let user: MatchingUser = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(user.user_id.as_deref()),
        or_dash(user.user_name.as_deref()),
        or_dash(user.user_email.as_deref()),
        or_dash(user.example_session_id.as_deref()),
        render::bool_display(Some(user.has_session_replay.unwrap_or(false))),
        or_dash(user.platform.as_deref()),
        or_dash(user.os.as_deref()),
        or_dash(user.country_name.as_deref()),
        or_dash(user.account_name.as_deref()),
    ])
}

fn pathway_row(item: &Value) -> Result<Vec<String>> {
    let pathway: PathwayResult = serde_json::from_value(item.clone())?;
    Ok(vec![
        count(pathway.count.as_deref()),
        pathway.pages.join(" -> "),
    ])
}

fn click_row(item: &Value) -> Result<Vec<String>> {
    let click: ClickResult = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(click.click_name.as_deref()),
        or_dash(click.cx_action_name.as_deref()),
        count(click.click_count.as_deref()),
        count(click.dead_click_count.as_deref()),
        count(click.rage_click_count.as_deref()),
    ])
}

fn saved_object_row(item: &Value) -> Result<Vec<String>> {
    let saved: SavedObject = serde_json::from_value(item.clone())?;
    Ok(vec![
        or_dash(saved.id.as_deref()),
        or_dash(saved.name.as_deref()),
        or_dash(saved.app_name.as_deref()),
        saved.details(),
        saved.created(),
    ])
}

const APPLICATION_HEADERS: &[&str] = &[
    "App",
    "Platform",
    "Sessions",
    "Crash-free sessions",
    "Users",
    "Crash-free users",
];
const VIEW_HEADERS: &[&str] = &["View", "Sessions"];
const ACTION_HEADERS: &[&str] = &["Type", "Target", "Sessions"];
const FILTER_VALUE_HEADERS: &[&str] = &["Value", "Count"];
const MATCHING_USER_HEADERS: &[&str] = &[
    "User ID",
    "Name",
    "Email",
    "Example session",
    "Replay",
    "Platform",
    "OS",
    "Country",
    "Account",
];
const PATHWAY_HEADERS: &[&str] = &["Sessions", "Path"];
const CLICK_HEADERS: &[&str] = &["Element", "Action name", "Clicks", "Dead", "Rage"];
const SAVED_OBJECT_HEADERS: &[&str] = &["ID", "Name", "App", "Details", "Created"];

fn render_items(
    results: Vec<(String, Vec<Value>)>,
    include_profile: bool,
    output: OutputFormat,
    headers: &[&str],
    row: RowRenderer,
    empty_message: &str,
) -> Result<()> {
    let mut all_json: Vec<Value> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (profile, items) in results {
        for mut item in items {
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
        OutputFormat::Toon => render::render_toon(&all_json)?,
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
    render_machine(output, &all_results)
}

fn print_funnel(response: &Value) -> Result<()> {
    let parsed: FunnelResponse = serde_json::from_value(response.clone())?;
    let with_groups = parsed.groups.len() > 1
        || parsed
            .groups
            .iter()
            .any(|group| group.group_value.as_deref().is_some_and(|v| !v.is_empty()));
    let rows: Vec<Vec<String>> = parsed
        .groups
        .iter()
        .flat_map(|group| {
            group.steps.iter().map(move |step| {
                let mut row = vec![String::new()];
                if with_groups {
                    row.push(or_dash(group.group_value.as_deref()));
                }
                row.extend([
                    step.step_number.to_string(),
                    count(step.session_count.as_deref()),
                    format!("{:.1}%", step.conversion_rate * 100.0),
                    or_dash(step.avg_time_from_prev_ms.as_deref()),
                ]);
                row
            })
        })
        .collect();
    if rows.is_empty() {
        render::print_no_results("No funnel result.");
        return Ok(());
    }
    let headers = [
        "Group",
        "Step",
        "Count",
        "Conversion",
        "Avg ms from previous",
    ];
    let headers = if with_groups {
        &headers[..]
    } else {
        &headers[1..]
    };
    render::render_table(headers, rows, false);
    Ok(())
}

fn print_scroll_map(response: &Value) -> Result<()> {
    let parsed: ScrollMapResponse = serde_json::from_value(response.clone())?;
    println!(
        "{}: {} px",
        "Average viewport height".bold(),
        parsed.avg_viewport_height
    );
    if parsed.page_covers.is_empty() {
        render::print_no_results("No scroll data found.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = parsed
        .page_covers
        .iter()
        .map(|cover| {
            vec![
                String::new(),
                format!("{}%", cover.page_percentage),
                count(cover.user_count.as_deref()),
            ]
        })
        .collect();
    render::render_table(&["Scroll depth", "Users"], rows, false);
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

async fn build_body(
    api: &ProductAnalyticsApi<'_>,
    kind: QueryKind,
    saved_id: Option<i64>,
    file_body: Option<Value>,
    input: &QueryInput,
) -> Result<Value> {
    let (Some(id), Some(saved_kind)) = (saved_id, kind.saved_kind()) else {
        let (Some(start), Some(end)) = (input.start, input.end) else {
            bail!("pass --start and --end");
        };
        return api::build_query_body(kind, file_body, input, (start, end));
    };
    let saved = api
        .get_saved(saved_kind, id)
        .await?
        .ok_or_else(|| anyhow!("{} {id} not found", saved_kind.label()))?;
    let (time_range, source) = match (input.start, input.end) {
        (Some(start), Some(end)) => ((start, end), "--start/--end".to_string()),
        (start, end) => {
            let ((saved_start, saved_end), label) =
                api::resolve_saved_time_range(&saved, chrono::Local::now())?;
            (
                (start.unwrap_or(saved_start), end.unwrap_or(saved_end)),
                format!("saved: {label}"),
            )
        }
    };
    let (start, end) = (
        format_local_time(time_range.0),
        format_local_time(time_range.1),
    );
    if time_range.1 <= time_range.0 {
        bail!("the end of the time range ({end}) must be after its start ({start})");
    }
    eprintln!(
        "{}",
        format!("Using time range {start} – {end} ({source})").dimmed()
    );
    api::build_query_body(
        kind,
        Some(api::request_from_saved(kind, &saved)),
        input,
        time_range,
    )
}

async fn run_queries(
    targets: &[Arc<ExecutionTarget>],
    kind: QueryKind,
    saved_id: Option<&str>,
    from_file: Option<&str>,
    input: &QueryInput,
) -> Result<Vec<(String, Value)>> {
    if let Some(id) = saved_id {
        require_single_target(targets, id)?;
    }
    let saved_id = saved_id.map(parse_id).transpose()?;
    let file_body = from_file.map(read_from_file).transpose()?;
    eprintln!("{}", "Running Product Analytics query...".dimmed());
    let per_profile = fan_out(targets, |t| {
        let file_body = file_body.clone();
        async move {
            let api = ProductAnalyticsApi::new(&t.client);
            let body = build_body(&api, kind, saved_id, file_body, input).await?;
            Ok(api.query(kind, &body).await?)
        }
    })
    .await;
    report_errors_and_collect_successes(per_profile)
}

pub async fn run_list_query(
    targets: &[Arc<ExecutionTarget>],
    kind: QueryKind,
    saved_id: Option<&str>,
    from_file: Option<&str>,
    input: &QueryInput,
    output: OutputFormat,
) -> Result<()> {
    let (headers, row, empty_message): (&[&str], RowRenderer, &str) = match kind {
        QueryKind::Applications => (
            APPLICATION_HEADERS,
            application_row,
            "No applications with sessions in the time range.",
        ),
        QueryKind::Views => (VIEW_HEADERS, view_row, "No views found."),
        QueryKind::Actions => (ACTION_HEADERS, action_row, "No actions found."),
        QueryKind::FilterValues => (FILTER_VALUE_HEADERS, filter_value_row, "No values found."),
        QueryKind::MatchingUsers => (
            MATCHING_USER_HEADERS,
            matching_user_row,
            "No matching users found.",
        ),
        QueryKind::Pathways => (PATHWAY_HEADERS, pathway_row, "No paths found."),
        QueryKind::Clicks => (CLICK_HEADERS, click_row, "No clicked elements found."),
        QueryKind::Funnel | QueryKind::ScrollMap => {
            unreachable!("{kind:?} is not a list query")
        }
    };
    let responses = run_queries(targets, kind, saved_id, from_file, input).await?;
    let results = responses
        .into_iter()
        .map(|(profile, response)| (profile, api::items(&response, kind.items_key())))
        .collect();
    render_items(
        results,
        targets.len() > 1,
        output,
        headers,
        row,
        empty_message,
    )
}

pub async fn run_query(
    targets: &[Arc<ExecutionTarget>],
    kind: QueryKind,
    saved_id: Option<&str>,
    from_file: Option<&str>,
    input: &QueryInput,
    output: OutputFormat,
) -> Result<()> {
    let text: fn(&Value) -> Result<()> = match kind {
        QueryKind::Funnel => print_funnel,
        QueryKind::ScrollMap => print_scroll_map,
        _ => unreachable!("{kind:?} is not a single-result query"),
    };
    let responses = run_queries(targets, kind, saved_id, from_file, input).await?;
    render_responses(responses, targets.len() > 1, output, text)
}

pub async fn run_saved_list(
    targets: &[Arc<ExecutionTarget>],
    kind: SavedObjectKind,
    app_names: &[String],
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching saved {}s...", kind.label()).dimmed()
    );
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        Ok(api.list_saved(kind, app_names).await?)
    })
    .await;
    render_items(
        report_errors_and_collect_successes(per_profile)?,
        targets.len() > 1,
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
    require_single_target(targets, id)?;
    let id = parse_id(id)?;
    eprintln!("{}", format!("Fetching {} {id}...", kind.label()).dimmed());
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        api.get_saved(kind, id)
            .await?
            .ok_or_else(|| anyhow!("{} {id} not found", kind.label()))
    })
    .await;
    let include_profile = targets.len() > 1;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut response) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut response, &profile);
        }
        all_results.push(response);
    }
    match output {
        OutputFormat::Text => render::render_get_text(
            &all_results,
            include_profile,
            &format!("{} {id} not found.", kind.label()),
            Some(
                &|response| match serde_json::from_value::<SavedObject>(response.clone()) {
                    Ok(saved) => {
                        println!("{}: {}", "Name".bold(), or_dash(saved.name.as_deref()));
                        println!("{}: {}", "ID".bold(), or_dash(saved.id.as_deref()));
                        println!("{}: {}", "App".bold(), or_dash(saved.app_name.as_deref()));
                        println!("{}: {}", "Details".bold(), saved.details());
                        println!("{}: {}", "Created".bold(), saved.created());
                    }
                    Err(error) => eprintln!("could not read the {} summary: {error}", kind.label()),
                },
            ),
        ),
        _ => render_machine(output, &all_results),
    }
}

pub async fn run_saved_write(
    targets: &[Arc<ExecutionTarget>],
    kind: SavedObjectKind,
    from_file: &str,
    update_id: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    if let Some(id) = update_id {
        require_single_target(targets, id)?;
    }
    let update_id = update_id.map(parse_id).transpose()?;
    let file_body = read_from_file(from_file)?;
    let name = file_body
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let body = match update_id {
        Some(_) => api::strip_read_only_fields(file_body)?,
        None => file_body,
    };
    let (progress, verb) = match update_id {
        Some(_) => ("Updating", "Updated"),
        None => ("Creating", "Created"),
    };
    eprintln!("{}", format!("{progress} {}...", kind.label()).dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ProductAnalyticsApi::new(&t.client);
            let response = match update_id {
                Some(id) => {
                    api.update_saved(kind, id, &body).await?;
                    json!({"id": id.to_string()})
                }
                None => api.create_saved(kind, &body).await?,
            };
            Ok(response)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut response) in report_errors_and_collect_successes(per_profile)? {
        let parsed: SavedObject = serde_json::from_value(response.clone())?;
        render::print_created(
            verb,
            kind.label(),
            name.as_deref(),
            parsed.id.as_deref(),
            &profile,
        );
        if include_profile {
            render::tag_get_result(&mut response, &profile);
        }
        all_results.push(response);
    }
    render_machine(output, &all_results)
}

pub async fn run_saved_delete(
    targets: &[Arc<ExecutionTarget>],
    kind: SavedObjectKind,
    id: &str,
) -> Result<()> {
    require_single_target(targets, id)?;
    let id = parse_id(id)?;
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
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        Ok(api.get_settings().await?)
    })
    .await;
    render_responses(
        report_errors_and_collect_successes(per_profile)?,
        targets.len() > 1,
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
    let per_profile = fan_out(targets, |t| async move {
        let api = ProductAnalyticsApi::new(&t.client);
        api.update_settings(enabled).await?;
        Ok(api.get_settings().await?)
    })
    .await;
    render_responses(
        report_errors_and_collect_successes(per_profile)?,
        targets.len() > 1,
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
    fn start_and_end_become_unix_seconds() {
        let args = time_range(Some("2026-11-16T00:00:00Z"), Some("2026-11-17T00:00:00Z"));
        assert_eq!(
            args.resolve(false).unwrap(),
            (Some(1_794_787_200), Some(1_794_873_600))
        );
    }

    #[test]
    fn no_flags_default_to_last_hour_unless_running_a_saved_object() {
        let (Some(start), Some(end)) = time_range(None, None).resolve(false).unwrap() else {
            panic!("both sides should default");
        };
        assert!((3_600..=3_601).contains(&(end - start)));
        assert_eq!(time_range(None, None).resolve(true).unwrap(), (None, None));
        let (start, end) = time_range(Some("now-1d"), None).resolve(true).unwrap();
        assert!(start.is_some());
        assert!(end.is_none());
        let (start, end) = time_range(None, Some("now-1d")).resolve(true).unwrap();
        assert!(start.is_none());
        assert!(end.is_some());
    }

    #[test]
    fn reversed_range_is_rejected() {
        assert!(time_range(Some("now-1h"), Some("now-2h"))
            .resolve(false)
            .is_err());
    }

    #[test]
    fn ids_must_be_positive_numbers() {
        assert_eq!(parse_id("42").unwrap(), 42);
        assert!(parse_id("0").is_err());
        assert!(parse_id("abc").is_err());
    }

    #[test]
    fn rows_have_one_cell_per_header() {
        let saved = saved_object_row(&json!({"id": "1", "name": "n"})).unwrap();
        assert_eq!(saved.len(), SAVED_OBJECT_HEADERS.len());
        let user = matching_user_row(&json!({"userId": "u1"})).unwrap();
        assert_eq!(user.len(), MATCHING_USER_HEADERS.len());
        let app = application_row(&json!({"appName": "web"})).unwrap();
        assert_eq!(app.len(), APPLICATION_HEADERS.len());
        let click = click_row(&json!({"clickName": "run"})).unwrap();
        assert_eq!(click.len(), CLICK_HEADERS.len());
    }
}
