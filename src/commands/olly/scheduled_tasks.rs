//! `cx olly scheduled-tasks`: manage Olly scheduled tasks, prompts Olly runs on a
//! schedule as the user who owns them.

use std::io::Read;
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use colored::Colorize;
use serde_json::{json, Map, Value};
use toon_format::encode_default as toon_encode;

use super::api::OllyApi;
use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::render;

const MIN_INTERVAL_SECONDS: u64 = 3600;
const MAX_INTERVAL_SECONDS: u64 = 7 * 24 * 3600;
const MAX_NOTIFICATION_CONNECTORS: usize = 5;
const MAX_PROMPT_BYTES: usize = 100 * 1024;
const PROMPT_PREVIEW_CHARS: usize = 80;

// ── Input parsing ─────────────────────────────────────────────────────────────

/// How often a task runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    Once,
    EverySeconds(u64),
    Monthly,
}

impl Cadence {
    /// Build a cadence from the mutually exclusive `--every` / `--monthly` /
    /// `--once` flags; `None` when none was given.
    pub fn from_flags(every: Option<&str>, monthly: bool, once: bool) -> Result<Option<Self>> {
        match (every, monthly, once) {
            (Some(e), false, false) => Ok(Some(Cadence::EverySeconds(parse_every(e)?))),
            (None, true, false) => Ok(Some(Cadence::Monthly)),
            (None, false, true) => Ok(Some(Cadence::Once)),
            (None, false, false) => Ok(None),
            _ => bail!("pass only one of --every, --monthly or --once"),
        }
    }

    fn interval_seconds(self) -> Value {
        match self {
            Cadence::EverySeconds(s) => json!(s),
            _ => Value::Null,
        }
    }

    fn interval_months(self) -> Value {
        match self {
            Cadence::Monthly => json!(1),
            _ => Value::Null,
        }
    }
}

/// Parse `--every` (`1h`, `6h`, `1d`, `1w`) into seconds. Olly accepts whole
/// hours from 1 hour to 1 week.
pub fn parse_every(input: &str) -> Result<u64> {
    let duration = humantime::parse_duration(input.trim())
        .map_err(|e| anyhow!("invalid --every '{input}': {e}. Examples: 1h, 6h, 1d, 1w"))?;
    let secs = duration.as_secs();
    if !(MIN_INTERVAL_SECONDS..=MAX_INTERVAL_SECONDS).contains(&secs) {
        bail!("--every must be between 1h and 1w, got '{input}'");
    }
    if secs % MIN_INTERVAL_SECONDS != 0 || duration.subsec_nanos() != 0 {
        bail!("--every must be a whole number of hours, got '{input}'");
    }
    Ok(secs)
}

/// Parse `--start-at`: `now`, `now+<duration>` (e.g. `now+2h`) or an ISO-8601
/// timestamp with a timezone.
pub fn parse_start_at(input: &str) -> Result<DateTime<Utc>> {
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("now") {
        return Ok(Utc::now());
    }
    if let Some(rest) = trimmed.strip_prefix("now") {
        let Some(duration) = rest.trim().strip_prefix('+') else {
            bail!("invalid --start-at '{input}': use now, now+2h, or an ISO-8601 timestamp");
        };
        let duration = humantime::parse_duration(duration.trim())
            .map_err(|e| anyhow!("invalid --start-at '{input}': {e}"))?;
        return Ok(Utc::now() + chrono::Duration::from_std(duration)?);
    }
    DateTime::parse_from_rfc3339(trimmed)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            anyhow!(
                "invalid --start-at '{input}': {e}. Use now, now+2h, or an ISO-8601 \
                 timestamp with a timezone, e.g. 2026-12-01T09:00:00Z"
            )
        })
}

fn format_start_at(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Read `--prompt`; `-` reads it from stdin. Blank prompts are rejected.
pub fn read_prompt(input: &str) -> Result<String> {
    let text = if input == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        input.to_string()
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        bail!("--prompt must not be empty");
    }
    if text.len() > MAX_PROMPT_BYTES {
        bail!("--prompt must be at most 100 KiB, got {} bytes", text.len());
    }
    Ok(text)
}

/// When a task's runs notify, from `--notify`.
pub fn build_notification(
    notify: &str,
    connector_ids: &[String],
    condition: Option<&str>,
) -> Result<Value> {
    if connector_ids.is_empty() {
        bail!("--notify needs at least one --connector <id> (an email or Slack connector)");
    }
    if connector_ids.len() > MAX_NOTIFICATION_CONNECTORS {
        bail!("at most {MAX_NOTIFICATION_CONNECTORS} --connector values are allowed");
    }
    let kind = match notify.trim().to_lowercase().replace('_', "-").as_str() {
        "every-run" => "every_run",
        "olly-judges" => "olly_judges",
        "on-condition" => "on_condition",
        _ => bail!("invalid --notify '{notify}': expected every-run, olly-judges or on-condition"),
    };
    let mut notification = json!({ "type": kind, "connector_ids": connector_ids });
    match (kind, condition.map(str::trim)) {
        ("on_condition", Some(c)) if !c.is_empty() => {
            notification["condition"] = json!(c);
        }
        ("on_condition", _) => bail!("--notify on-condition needs --condition \"<text>\""),
        (_, Some(_)) => bail!("--condition applies only to --notify on-condition"),
        _ => {}
    }
    Ok(notification)
}

fn shared_options(team: bool) -> Value {
    if team {
        json!({ "shared_type": "entire_organization" })
    } else {
        json!({ "shared_type": "private" })
    }
}

/// Fields of a new task, as given on the command line.
pub struct CreateTaskArgs<'a> {
    pub title: &'a str,
    pub prompt: &'a str,
    pub model: &'a str,
    pub reasoning_effort: Option<&'a str>,
    pub start_at: &'a str,
    pub cadence: Cadence,
    pub disabled: bool,
    pub notify: Option<&'a str>,
    pub connector_ids: &'a [String],
    pub condition: Option<&'a str>,
    pub share_with_team: bool,
}

/// A monthly task repeats on its first run's UTC day of the month, and a run
/// on the 29th-31st drifts to an earlier day after a shorter month.
fn check_monthly_start(cadence: Cadence, start_at: DateTime<Utc>) -> Result<()> {
    use chrono::Datelike;
    if cadence == Cadence::Monthly && start_at.day() > 28 {
        bail!(
            "a monthly task repeats on its first run's UTC day of the month, and runs on \
             the 29th-31st drift to an earlier day; pick a --start-at on the 1st-28th (UTC)"
        );
    }
    Ok(())
}

pub fn build_create_body(args: &CreateTaskArgs<'_>) -> Result<Value> {
    if args.title.trim().is_empty() {
        bail!("--title must not be empty");
    }
    let start_at = parse_start_at(args.start_at)?;
    check_monthly_start(args.cadence, start_at)?;
    let mut body = json!({
        "title": args.title.trim(),
        "prompt": read_prompt(args.prompt)?,
        "model_choice": args.model,
        "start_at": format_start_at(start_at),
        "interval_seconds": args.cadence.interval_seconds(),
        "interval_months": args.cadence.interval_months(),
        "enabled": !args.disabled,
        "shared_options": shared_options(args.share_with_team),
    });
    if let Some(effort) = args.reasoning_effort {
        body["reasoning_effort"] = json!(effort);
    }
    match args.notify {
        Some(n) => {
            body["notification"] = build_notification(n, args.connector_ids, args.condition)?
        }
        None if !args.connector_ids.is_empty() || args.condition.is_some() => {
            bail!("--connector and --condition need --notify")
        }
        None => {}
    }
    Ok(body)
}

/// Content changes for `update`, as given on the command line.
pub struct UpdateTaskArgs<'a> {
    pub title: Option<&'a str>,
    pub prompt: Option<&'a str>,
    pub model: Option<&'a str>,
    pub reasoning_effort: Option<&'a str>,
    pub notify: Option<&'a str>,
    pub connector_ids: &'a [String],
    pub condition: Option<&'a str>,
    pub no_notify: bool,
    pub start_at: Option<&'a str>,
    pub cadence: Option<Cadence>,
}

/// The PATCH body for content changes, or `None` when only the schedule changes.
pub fn build_content_patch(args: &UpdateTaskArgs<'_>) -> Result<Option<Value>> {
    let mut patch = Map::new();
    if let Some(t) = args.title {
        if t.trim().is_empty() {
            bail!("--title must not be empty");
        }
        patch.insert("title".into(), json!(t.trim()));
    }
    if let Some(p) = args.prompt {
        patch.insert("prompt".into(), json!(read_prompt(p)?));
    }
    if let Some(m) = args.model {
        patch.insert("model_choice".into(), json!(m));
    }
    if let Some(e) = args.reasoning_effort {
        patch.insert("reasoning_effort".into(), json!(e));
    }
    match (args.notify, args.no_notify) {
        (Some(_), true) => bail!("pass either --notify or --no-notify, not both"),
        (Some(n), false) => {
            patch.insert(
                "notification".into(),
                build_notification(n, args.connector_ids, args.condition)?,
            );
        }
        (None, true) => {
            patch.insert("notification".into(), Value::Null);
        }
        (None, false) if !args.connector_ids.is_empty() || args.condition.is_some() => {
            bail!("--connector and --condition need --notify")
        }
        (None, false) => {}
    }
    Ok((!patch.is_empty()).then_some(Value::Object(patch)))
}

/// The full schedule body for PUT `/schedule`, keeping whatever the flags do
/// not change from the task's current schedule.
pub fn build_schedule_body(
    current: &Value,
    start_at: Option<&str>,
    cadence: Option<Cadence>,
    enabled: Option<bool>,
) -> Result<Value> {
    let start_at = match start_at {
        Some(s) => {
            let parsed = parse_start_at(s)?;
            if let Some(c) = cadence {
                check_monthly_start(c, parsed)?;
            }
            format_start_at(parsed)
        }
        None => current
            .get("start_at")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("the task has no start_at; pass --start-at"))?
            .to_string(),
    };
    let (interval_seconds, interval_months) = match cadence {
        Some(c) => (c.interval_seconds(), c.interval_months()),
        None => (
            current
                .get("interval_seconds")
                .cloned()
                .unwrap_or(Value::Null),
            current
                .get("interval_months")
                .cloned()
                .unwrap_or(Value::Null),
        ),
    };
    let enabled = enabled
        .or_else(|| current.get("enabled").and_then(Value::as_bool))
        .unwrap_or(true);
    Ok(json!({
        "start_at": start_at,
        "interval_seconds": interval_seconds,
        "interval_months": interval_months,
        "enabled": enabled,
    }))
}

/// Task IDs go into the URL path, so only UUIDs are accepted.
pub fn validate_task_id(id: &str) -> Result<String> {
    uuid::Uuid::parse_str(id.trim())
        .map(|u| u.hyphenated().to_string())
        .map_err(|_| anyhow!("invalid task ID '{id}': expected a UUID"))
}

// ── Display helpers ───────────────────────────────────────────────────────────

/// `every 6h`, `every 1d`, `monthly` or `once`.
pub fn describe_cadence(task: &Value) -> String {
    if let Some(secs) = task.get("interval_seconds").and_then(Value::as_u64) {
        let hours = secs / 3600;
        return if hours % 168 == 0 {
            format!("every {}w", hours / 168)
        } else if hours % 24 == 0 {
            format!("every {}d", hours / 24)
        } else {
            format!("every {hours}h")
        };
    }
    if task
        .get("interval_months")
        .and_then(Value::as_u64)
        .is_some()
    {
        return "monthly".into();
    }
    "once".into()
}

fn describe_sharing(task: &Value) -> String {
    match task
        .pointer("/shared_options/shared_type")
        .and_then(Value::as_str)
    {
        Some("entire_organization") => "team".into(),
        Some("specific_entities") => "specific users".into(),
        _ => "private".into(),
    }
}

fn describe_notification(task: &Value) -> String {
    let Some(n) = task.get("notification").filter(|v| !v.is_null()) else {
        return "none".into();
    };
    let kind = n.get("type").and_then(Value::as_str).unwrap_or("-");
    let count = n
        .get("connector_ids")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    match n.get("condition").and_then(Value::as_str) {
        Some(c) => format!("{kind} ({count} connector(s)): {c}"),
        None => format!("{kind} ({count} connector(s))"),
    }
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or("-")
        .to_string()
}

fn preview(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > PROMPT_PREVIEW_CHARS {
        let cut: String = one_line.chars().take(PROMPT_PREVIEW_CHARS).collect();
        format!("{cut}…")
    } else {
        one_line
    }
}

fn print_values(values: &Value, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(values)?),
        OutputFormat::Toon => {
            let toon = toon_encode(values).map_err(|e| anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }
    Ok(())
}

fn single_api(targets: &[Arc<ExecutionTarget>]) -> Result<OllyApi> {
    if targets.len() > 1 {
        bail!("cx olly does not support multi-profile. Use a single profile with -p <profile>.");
    }
    Ok(OllyApi::from_client(targets[0].client.clone()))
}

fn print_task_text(task: &Value) {
    println!("{}:        {}", "Title".bold(), str_field(task, "title"));
    println!("{}:           {}", "ID".bold(), str_field(task, "id"));
    println!(
        "{}:      {}",
        "Enabled".bold(),
        task.get("enabled").and_then(Value::as_bool).unwrap_or(true)
    );
    println!("{}:     {}", "Schedule".bold(), describe_cadence(task));
    println!("{}:     {}", "Start at".bold(), str_field(task, "start_at"));
    println!(
        "{}:     {}",
        "Next run".bold(),
        str_field(task, "next_run_at")
    );
    println!(
        "{}:     {}",
        "Last run".bold(),
        str_field(task, "last_run_at")
    );
    println!(
        "{}:        {}",
        "Model".bold(),
        str_field(task, "model_choice")
    );
    println!("{}:       {}", "Shared".bold(), describe_sharing(task));
    println!("{}:       {}", "Notify".bold(), describe_notification(task));
    let owner = task
        .pointer("/owner/email")
        .and_then(Value::as_str)
        .unwrap_or("-");
    println!("{}:        {owner}", "Owner".bold());
    println!("{}:\n{}", "Prompt".bold(), str_field(task, "prompt"));
}

fn write_result(task: &Value, message: &str, output: OutputFormat) -> Result<()> {
    eprintln!("{}", message.green());
    print_values(task, output)
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    shared: bool,
    output: OutputFormat,
) -> Result<()> {
    let api = single_api(targets)?;
    let label = if shared { "shared with you" } else { "you own" };
    eprintln!(
        "{}",
        format!("Fetching scheduled tasks {label}...").dimmed()
    );
    let tasks = api.list_scheduled_tasks(shared).await?;

    if output != OutputFormat::Text {
        return print_values(&Value::Array(tasks), output);
    }
    if tasks.is_empty() {
        render::print_no_results("No scheduled tasks found.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = tasks
        .iter()
        .map(|t| {
            let mut row = vec![
                String::new(),
                str_field(t, "id"),
                str_field(t, "title"),
                describe_cadence(t),
                t.get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                    .to_string(),
                str_field(t, "next_run_at"),
                describe_sharing(t),
            ];
            if shared {
                row.push(
                    t.pointer("/owner/email")
                        .and_then(Value::as_str)
                        .unwrap_or("-")
                        .to_string(),
                );
            }
            row
        })
        .collect();
    let mut headers = vec!["ID", "Title", "Schedule", "Enabled", "Next run", "Shared"];
    if shared {
        headers.push("Owner");
    }
    render::render_table(&headers, rows, false);
    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    output: OutputFormat,
) -> Result<()> {
    let id = validate_task_id(task_id)?;
    let api = single_api(targets)?;
    eprintln!("{}", format!("Fetching scheduled task {id}...").dimmed());
    let task = api.get_scheduled_task(&id).await?;
    if output == OutputFormat::Text {
        print_task_text(&task);
        return Ok(());
    }
    print_values(&task, output)
}

pub async fn run_runs(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    output: OutputFormat,
) -> Result<()> {
    let id = validate_task_id(task_id)?;
    let api = single_api(targets)?;
    eprintln!(
        "{}",
        format!("Fetching runs of scheduled task {id}...").dimmed()
    );
    let runs = api.list_scheduled_task_runs(&id).await?;

    if output != OutputFormat::Text {
        return print_values(&Value::Array(runs), output);
    }
    if runs.is_empty() {
        render::print_no_results("No runs yet.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = runs
        .iter()
        .map(|r| {
            let outcome = r
                .pointer("/notification_verdict/reason")
                .and_then(Value::as_str)
                .map(preview)
                .or_else(|| {
                    r.get("error_reason")
                        .and_then(Value::as_str)
                        .map(String::from)
                })
                .unwrap_or_default();
            vec![
                String::new(),
                str_field(r, "started_at"),
                str_field(r, "status"),
                str_field(r, "finished_at"),
                str_field(r, "chat_id"),
                outcome,
            ]
        })
        .collect();
    render::render_table(
        &[
            "Started",
            "Status",
            "Finished",
            "Chat ID",
            "Verdict / error",
        ],
        rows,
        false,
    );
    Ok(())
}

pub async fn run_create(
    targets: &[Arc<ExecutionTarget>],
    args: &CreateTaskArgs<'_>,
    output: OutputFormat,
) -> Result<()> {
    let body = build_create_body(args)?;
    let api = single_api(targets)?;
    eprintln!(
        "{}",
        format!("Creating scheduled task '{}'...", args.title.trim()).dimmed()
    );
    let task = api.create_scheduled_task(&body).await?;
    let mut message = format!(
        "Created scheduled task {} ({}, next run {}).",
        str_field(&task, "id"),
        describe_cadence(&task),
        str_field(&task, "next_run_at")
    );
    if describe_sharing(&task) == "team" && !args.share_with_team {
        message.push_str(
            " It is shared with the whole team because its prompt uses /case-management.",
        );
    }
    write_result(&task, &message, output)
}

pub async fn run_update(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    args: &UpdateTaskArgs<'_>,
    output: OutputFormat,
) -> Result<()> {
    let id = validate_task_id(task_id)?;
    if args.cadence == Some(Cadence::Once) && args.start_at.is_none() {
        bail!("--once needs --start-at: a one-off task runs once, at that time");
    }
    let patch = build_content_patch(args)?;
    let changes_schedule = args.start_at.is_some() || args.cadence.is_some();
    if patch.is_none() && !changes_schedule {
        bail!(
            "nothing to update: pass --title, --prompt, --model, --reasoning-effort, \
             --notify/--no-notify, --start-at, --every, --monthly or --once"
        );
    }
    let api = single_api(targets)?;
    eprintln!("{}", format!("Updating scheduled task {id}...").dimmed());

    let mut task = Value::Null;
    if let Some(patch) = patch {
        task = api.update_scheduled_task_content(&id, &patch).await?;
    }
    if changes_schedule {
        let current = if task.is_null() {
            api.get_scheduled_task(&id).await?
        } else {
            task
        };
        let body = build_schedule_body(&current, args.start_at, args.cadence, None)?;
        task = api.replace_scheduled_task_schedule(&id, &body).await?;
    }
    let mut message = format!("Updated scheduled task {id}.");
    if args.prompt.is_some() && describe_sharing(&task) == "team" {
        message.push_str(" Its prompt uses /case-management, so it is shared with the team.");
    }
    write_result(&task, &message, output)
}

pub async fn run_set_enabled(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    enabled: bool,
    output: OutputFormat,
) -> Result<()> {
    let id = validate_task_id(task_id)?;
    let api = single_api(targets)?;
    let verb = if enabled { "Enabling" } else { "Disabling" };
    eprintln!("{}", format!("{verb} scheduled task {id}...").dimmed());
    let current = api.get_scheduled_task(&id).await?;
    let body = build_schedule_body(&current, None, None, Some(enabled))?;
    let task = api.replace_scheduled_task_schedule(&id, &body).await?;
    let done = if enabled { "Enabled" } else { "Disabled" };
    write_result(&task, &format!("{done} scheduled task {id}."), output)
}

pub async fn run_share(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    team: bool,
    output: OutputFormat,
) -> Result<()> {
    let id = validate_task_id(task_id)?;
    let api = single_api(targets)?;
    eprintln!(
        "{}",
        format!("Changing sharing of scheduled task {id}...").dimmed()
    );
    let task = api
        .set_scheduled_task_sharing(&id, &shared_options(team))
        .await?;
    let who = if team {
        "shared with the whole team (task, run history and run chats)"
    } else {
        "private"
    };
    write_result(&task, &format!("Scheduled task {id} is now {who}."), output)
}

pub async fn run_delete(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    output: OutputFormat,
) -> Result<()> {
    let id = validate_task_id(task_id)?;
    let api = single_api(targets)?;
    eprintln!("{}", format!("Deleting scheduled task {id}...").dimmed());
    api.delete_scheduled_task(&id).await?;
    write_result(
        &json!({ "id": id, "deleted": true }),
        &format!("Deleted scheduled task {id}. Its run chats are kept."),
        output,
    )
}

pub async fn run_now(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    output: OutputFormat,
) -> Result<()> {
    let id = validate_task_id(task_id)?;
    let api = single_api(targets)?;
    eprintln!(
        "{}",
        format!("Starting a run of scheduled task {id}...").dimmed()
    );
    api.run_scheduled_task(&id).await?;
    write_result(
        &json!({ "id": id, "run_started": true }),
        &format!(
            "Started a run of scheduled task {id}. Follow it with `cx olly scheduled-tasks runs {id}`."
        ),
        output,
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_every_accepts_whole_hours_within_bounds() {
        assert_eq!(parse_every("1h").unwrap(), 3600);
        assert_eq!(parse_every("6h").unwrap(), 6 * 3600);
        assert_eq!(parse_every("1d").unwrap(), 86_400);
        assert_eq!(parse_every("1w").unwrap(), 7 * 86_400);
    }

    #[test]
    fn parse_every_rejects_out_of_range_and_partial_hours() {
        assert!(parse_every("30m").is_err());
        assert!(parse_every("8d").is_err());
        assert!(parse_every("90m").is_err());
        assert!(parse_every("soon").is_err());
    }

    #[test]
    fn cadence_flags_are_mutually_exclusive() {
        assert_eq!(Cadence::from_flags(None, false, false).unwrap(), None);
        assert_eq!(
            Cadence::from_flags(Some("2h"), false, false).unwrap(),
            Some(Cadence::EverySeconds(7200))
        );
        assert_eq!(
            Cadence::from_flags(None, true, false).unwrap(),
            Some(Cadence::Monthly)
        );
        assert!(Cadence::from_flags(Some("2h"), true, false).is_err());
    }

    #[test]
    fn parse_start_at_accepts_now_plus_and_rfc3339() {
        let soon = parse_start_at("now+2h").unwrap();
        assert!(soon > Utc::now() + chrono::Duration::minutes(119));
        let fixed = parse_start_at("2026-12-01T09:00:00+02:00").unwrap();
        assert_eq!(format_start_at(fixed), "2026-12-01T07:00:00Z");
        assert!(parse_start_at("now-1h").is_err());
        assert!(parse_start_at("tomorrow").is_err());
    }

    #[test]
    fn notification_requires_connectors_and_a_condition_only_for_on_condition() {
        let ids = vec!["c1".to_string()];
        assert_eq!(
            build_notification("every-run", &ids, None).unwrap(),
            json!({ "type": "every_run", "connector_ids": ["c1"] })
        );
        assert_eq!(
            build_notification("on-condition", &ids, Some("error rate above 5%")).unwrap(),
            json!({
                "type": "on_condition",
                "connector_ids": ["c1"],
                "condition": "error rate above 5%"
            })
        );
        assert!(build_notification("every-run", &[], None).is_err());
        assert!(build_notification("on-condition", &ids, None).is_err());
        assert!(build_notification("olly-judges", &ids, Some("x")).is_err());
        assert!(build_notification("sometimes", &ids, None).is_err());
    }

    fn create_args(cadence: Cadence) -> CreateTaskArgs<'static> {
        CreateTaskArgs {
            title: " Checkout errors ",
            prompt: "Check checkout errors",
            model: "gpt-5.2",
            reasoning_effort: None,
            start_at: "2026-12-01T09:00:00Z",
            cadence,
            disabled: false,
            notify: None,
            connector_ids: &[],
            condition: None,
            share_with_team: false,
        }
    }

    #[test]
    fn create_body_sets_the_schedule_and_private_sharing() {
        let body = build_create_body(&create_args(Cadence::EverySeconds(3600))).unwrap();
        assert_eq!(body["title"], "Checkout errors");
        assert_eq!(body["start_at"], "2026-12-01T09:00:00Z");
        assert_eq!(body["interval_seconds"], 3600);
        assert_eq!(body["interval_months"], Value::Null);
        assert_eq!(body["enabled"], true);
        assert_eq!(body["shared_options"]["shared_type"], "private");
        assert!(body.get("notification").is_none());

        let monthly = build_create_body(&create_args(Cadence::Monthly)).unwrap();
        assert_eq!(monthly["interval_seconds"], Value::Null);
        assert_eq!(monthly["interval_months"], 1);
    }

    #[test]
    fn a_monthly_task_cannot_start_on_the_29th_to_31st() {
        let mut args = create_args(Cadence::Monthly);
        args.start_at = "2026-12-30T09:00:00Z";
        assert!(build_create_body(&args).is_err());
        args.cadence = Cadence::EverySeconds(86400);
        assert!(build_create_body(&args).is_ok());
    }

    #[test]
    fn create_body_rejects_connectors_without_notify() {
        let ids = vec!["c1".to_string()];
        let mut args = create_args(Cadence::Once);
        args.connector_ids = &ids;
        assert!(build_create_body(&args).is_err());
    }

    fn update_args() -> UpdateTaskArgs<'static> {
        UpdateTaskArgs {
            title: None,
            prompt: None,
            model: None,
            reasoning_effort: None,
            notify: None,
            connector_ids: &[],
            condition: None,
            no_notify: false,
            start_at: None,
            cadence: None,
        }
    }

    #[test]
    fn content_patch_only_holds_given_fields() {
        assert!(build_content_patch(&update_args()).unwrap().is_none());

        let mut args = update_args();
        args.title = Some("New title");
        args.no_notify = true;
        assert_eq!(
            build_content_patch(&args).unwrap().unwrap(),
            json!({ "title": "New title", "notification": null })
        );
    }

    #[test]
    fn schedule_body_keeps_what_the_flags_leave_alone() {
        let current = json!({
            "start_at": "2026-11-01T09:00:00Z",
            "interval_seconds": 86400,
            "interval_months": null,
            "enabled": true
        });
        assert_eq!(
            build_schedule_body(&current, None, None, Some(false)).unwrap(),
            json!({
                "start_at": "2026-11-01T09:00:00Z",
                "interval_seconds": 86400,
                "interval_months": null,
                "enabled": false
            })
        );
        assert_eq!(
            build_schedule_body(&current, None, Some(Cadence::Monthly), None).unwrap(),
            json!({
                "start_at": "2026-11-01T09:00:00Z",
                "interval_seconds": null,
                "interval_months": 1,
                "enabled": true
            })
        );
    }

    #[test]
    fn task_ids_must_be_uuids() {
        assert_eq!(
            validate_task_id("11111111222243338444555555555555").unwrap(),
            "11111111-2222-4333-8444-555555555555"
        );
        assert!(validate_task_id("").is_err());
        assert!(validate_task_id("task-1").is_err());
    }

    #[test]
    fn cadence_is_described_in_the_largest_whole_unit() {
        assert_eq!(
            describe_cadence(&json!({ "interval_seconds": 7200 })),
            "every 2h"
        );
        assert_eq!(
            describe_cadence(&json!({ "interval_seconds": 172800 })),
            "every 2d"
        );
        assert_eq!(
            describe_cadence(&json!({ "interval_seconds": 604800 })),
            "every 1w"
        );
        assert_eq!(
            describe_cadence(&json!({ "interval_months": 1 })),
            "monthly"
        );
        assert_eq!(describe_cadence(&json!({})), "once");
    }
}
