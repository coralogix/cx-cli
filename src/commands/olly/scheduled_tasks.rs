use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use colored::Colorize;
use serde_json::{json, Map, Value};

use super::api::{OllyApi, ScheduledTask, ScheduledTaskRun, TaskSchedule};
use crate::config::OutputFormat;
use crate::error::CxError;
use crate::execution::ExecutionTarget;
use crate::render;
use crate::spill::{maybe_spill, SpillOutcome};
use crate::time::parse_timestamp_allowing_future;

const HOUR_SECONDS: u64 = 3600;
const MIN_INTERVAL_SECONDS: u64 = HOUR_SECONDS;
const MAX_INTERVAL_SECONDS: u64 = 7 * 24 * HOUR_SECONDS;
const DISABLED_NOTE: &str = " The task is disabled, so it will not run until you enable it.";
/// Team API keys start with this. Personal keys start with `cxup_`.
const TEAM_KEY_PREFIX: &str = "cxtp_";
/// Each `--with` value and the API's `shared_type` for it.
const SHARED_TYPES: [(&str, &str); 3] = [
    ("private", "private"),
    ("org", "entire_organization"),
    ("entities", "specific_entities"),
];

/// What `PATCH /{task_id}` changes. It ignores every other key.
const UPDATE_FIELDS: [&str; 5] = [
    "title",
    "prompt",
    "model_choice",
    "reasoning_effort",
    "notification",
];
/// What `POST /` reads besides [`UPDATE_FIELDS`].
const CREATE_ONLY_FIELDS: [&str; 7] = [
    "start_at",
    "interval_seconds",
    "interval_months",
    "enabled",
    "kind",
    "metadata",
    "shared_options",
];
/// What a task from `get -o json` has that no request reads. A create file
/// may keep these, so that a saved task can be created again.
const READ_ONLY_FIELDS: [&str; 9] = [
    "id",
    "team_id",
    "owner",
    "is_owner",
    "next_run_at",
    "last_run_at",
    "created_at",
    "updated_at",
    "scheduled_task_type",
];

/// The title, prompt and model flags of `create` and `update`.
#[derive(Debug, Default)]
pub struct TaskContentFlags {
    pub title: Option<String>,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

impl TaskContentFlags {
    /// The API fields for the flags that were given.
    fn into_body(self) -> Map<String, Value> {
        [
            ("title", self.title),
            ("prompt", self.prompt),
            ("model_choice", self.model),
            ("reasoning_effort", self.reasoning_effort),
        ]
        .into_iter()
        .filter_map(|(key, value)| Some((key.to_string(), Value::String(value?))))
        .collect()
    }
}

/// The schedule flags of `create` and `set-schedule`. Neither `every` nor
/// `monthly` makes the task a one-off.
#[derive(Debug)]
pub struct ScheduleFlags {
    /// `--start-at` as typed. It is resolved when the request is built, so
    /// `now+5m` does not count the time spent at a confirmation prompt.
    pub start_at: String,
    /// `--every`, in seconds (from [`parse_every`]).
    pub every: Option<u64>,
    pub monthly: bool,
}

impl ScheduleFlags {
    fn into_schedule(self, enabled: bool) -> Result<TaskSchedule> {
        Ok(TaskSchedule {
            start_at: parse_timestamp_allowing_future(&self.start_at)?,
            interval_seconds: self.every,
            interval_months: self.monthly.then_some(1),
            enabled,
        })
    }
}

/// `--every` as seconds. The API runs tasks on whole hours between one hour
/// and one week, so anything else fails here, before a request is sent.
pub fn parse_every(input: &str) -> std::result::Result<u64, String> {
    let seconds = humantime::parse_duration(input.trim())
        .map_err(|e| format!("{e}. Examples: '1h', '6h', '1d', '1w'"))?
        .as_secs();
    if !(MIN_INTERVAL_SECONDS..=MAX_INTERVAL_SECONDS).contains(&seconds) {
        return Err("must be between 1 hour and 1 week".to_string());
    }
    if !seconds.is_multiple_of(HOUR_SECONDS) {
        return Err("must be a whole number of hours, for example '1h', '6h' or '1d'".to_string());
    }
    Ok(seconds)
}

/// Check `--start-at` when the flags are parsed and keep it as typed; see
/// [`ScheduleFlags::start_at`].
pub fn parse_start_at(input: &str) -> std::result::Result<String, String> {
    parse_timestamp_allowing_future(input)
        .map(|_| input.to_string())
        .map_err(|_| {
            "use 'now+1h', 'now+3d' or an ISO-8601 time with a timezone \
             (for example '2026-12-01T09:00:00Z')"
                .to_string()
        })
}

/// A task or dashboard ID as it goes into the request path. IDs are UUIDs or
/// short slugs; anything else (`../`, `?`) could reach a different endpoint
/// than the one the user confirmed.
pub fn parse_id(input: &str) -> std::result::Result<String, String> {
    let valid = !input.is_empty()
        && input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(input.to_string())
    } else {
        Err("an ID has only letters, digits, '-' and '_'".to_string())
    }
}

/// A scheduled task belongs to one user, so the commands take one profile and
/// refuse a team API key before any request. The API would answer that key
/// with a 403 on every endpoint.
fn tasks_api(targets: &[Arc<ExecutionTarget>]) -> Result<OllyApi> {
    if targets.len() > 1 {
        bail!("cx olly does not support multi-profile. Use a single profile with -p <profile>.");
    }
    if targets[0].cfg.api_key.starts_with(TEAM_KEY_PREFIX) {
        bail!(
            "Scheduled tasks need a user identity, and this profile uses a team API key \
             ({TEAM_KEY_PREFIX}...).\n\
             Use a personal API key or a logged-in profile."
        );
    }
    Ok(OllyApi::from_client(targets[0].client.clone()))
}

/// The API's `shared_type` for a `--with` value.
fn api_shared_type(with: &str) -> Result<&'static str> {
    SHARED_TYPES
        .iter()
        .find(|(cli, _)| *cli == with)
        .map(|(_, api)| *api)
        .ok_or_else(|| anyhow!("unknown --with value '{with}'"))
}

/// The `--with` value for an API `shared_type`, so `get` shows what `share`
/// takes. A value this CLI does not know is shown as the API sent it.
fn with_value(shared_type: &str) -> &str {
    SHARED_TYPES
        .iter()
        .find(|(_, api)| *api == shared_type)
        .map_or(shared_type, |(cli, _)| cli)
}

/// Fail on the keys of a task file that the API would drop without an error,
/// so a misspelled key cannot look like a change. `hint` says where those
/// keys belong.
fn reject_ignored_keys(ignored: &[&str], hint: &str) -> Result<()> {
    if ignored.is_empty() {
        return Ok(());
    }
    bail!(
        "The file has keys this command does not read: {}.{hint}",
        ignored.join(", ")
    );
}

/// The keys of a task file that the API would drop without an error. `known`
/// is every key the request reads; `silent` is keys to leave unmentioned.
fn ignored_keys<'a>(body: &'a Value, known: &[&[&str]], silent: &[&str]) -> Vec<&'a str> {
    let mut keys: Vec<&str> = body
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .map(String::as_str)
        .filter(|key| !silent.contains(key) && !known.iter().any(|keys| keys.contains(key)))
        .collect();
    keys.sort_unstable();
    keys
}

/// Read a JSON object from a file, or from stdin for `-`.
fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading task from stdin...".dimmed());
        std::io::read_to_string(std::io::stdin())?
    } else {
        eprintln!("{}", format!("Reading task from {path}...").dimmed());
        std::fs::read_to_string(path)?
    };
    let body: Value = serde_json::from_str(&raw)?;
    if !body.is_object() {
        bail!("The task file must hold one JSON object.");
    }
    Ok(body)
}

/// The API answers a team API key with a 403 that says what is refused but
/// not what to use instead. [`tasks_api`] stops the keys it can recognise
/// first; this covers the rest. Other 403s (AI consent is off, a missing
/// permission) are left as the API wrote them.
fn explain(error: CxError) -> anyhow::Error {
    match error {
        CxError::Permission(detail) if detail.contains("user identity") => anyhow!(
            "Permission denied: {detail}\n\
             Use a personal API key or a logged-in profile."
        ),
        other => other.into(),
    }
}

/// [`explain`], plus the two answers of the write endpoints that name no
/// cause: a 404 for a task the caller can read but does not own, and a 409
/// for a title (or a watched dashboard) that is already taken.
fn explain_write(error: CxError) -> anyhow::Error {
    match error {
        CxError::Api {
            status: 404,
            message,
        } => anyhow!(
            "{message}\n\
             The task does not exist, or you are not its owner. Only the owner can \
             change, run, share or delete a task."
        ),
        CxError::Api {
            status: 409,
            message,
        } if is_duplicate(&message) => anyhow!(
            "A scheduled task with this title already exists, or one already watches \
             this dashboard. (API: {message})"
        ),
        other => explain(other),
    }
}

/// [`explain_write`], plus the 409 the API answers when the user already
/// owns the maximum number of tasks. The API's message names the limit.
fn explain_create(error: CxError) -> anyhow::Error {
    match error {
        CxError::Api {
            status: 409,
            message,
        } if !is_duplicate(&message) => anyhow!(
            "Scheduled task quota reached: {message}\n\
             `cx olly scheduled-tasks list` shows your tasks."
        ),
        other => explain_write(other),
    }
}

/// The API's generic 409 body for a unique-constraint violation.
fn is_duplicate(message: &str) -> bool {
    message.to_ascii_lowercase().contains("already exists")
}

/// Load a task before a write. Writes are owner-only and the API answers a
/// non-owner with a bare 404, so this says whose task it is instead.
async fn owned_task(api: &OllyApi, task_id: &str) -> Result<(Value, ScheduledTask)> {
    let raw = api.get_scheduled_task(task_id).await.map_err(explain)?;
    let task = parse_task(&raw)?;
    ensure_owner(&task)?;
    Ok((raw, task))
}

fn ensure_owner(task: &ScheduledTask) -> Result<()> {
    if task.is_owner == Some(false) {
        let owner = task
            .owner
            .as_ref()
            .and_then(|owner| owner.email.as_ref().or(owner.entity_id.as_ref()));
        bail!(
            "Scheduled task {} belongs to {}. Only the owner can change it.",
            task.id,
            owner.map_or("another user", String::as_str)
        );
    }
    Ok(())
}

fn or_dash(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("-")
}

/// `every 6h`, `every 2d`, `monthly` or `once`.
fn schedule_label(interval_seconds: Option<u64>, interval_months: Option<u32>) -> String {
    match (interval_seconds, interval_months) {
        (Some(seconds), _) => format!("every {}", interval_label(seconds)),
        (None, Some(1)) => "monthly".to_string(),
        (None, Some(months)) => format!("every {months} months"),
        (None, None) => "once".to_string(),
    }
}

/// The largest whole unit: `1w`, `2d`, `6h`. Seconds when it is not a whole
/// number of hours, which the API does not create today.
fn interval_label(seconds: u64) -> String {
    let hours = seconds / HOUR_SECONDS;
    if !seconds.is_multiple_of(HOUR_SECONDS) {
        format!("{seconds}s")
    } else if hours.is_multiple_of(7 * 24) {
        format!("{}w", hours / (7 * 24))
    } else if hours.is_multiple_of(24) {
        format!("{}d", hours / 24)
    } else {
        format!("{hours}h")
    }
}

fn parse_task(task: &Value) -> Result<ScheduledTask> {
    Ok(serde_json::from_value(task.clone())?)
}

/// Print a task a write command got back: the full task for `-o json` and
/// `-o toon`, nothing for text, where the status line on stderr is enough.
fn print_written_task(task: Value, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json_auto(&[task]),
        OutputFormat::Toon => render::render_toon(&[task]),
        OutputFormat::Text => Ok(()),
    }
}

/// List the caller's tasks, or the tasks shared with them.
pub async fn run_tasks_list(
    targets: &[Arc<ExecutionTarget>],
    kind: Option<&str>,
    shared: bool,
    dashboard_id: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let api = tasks_api(targets)?;
    eprintln!("{}", "Fetching scheduled tasks...".dimmed());
    let tasks = api
        .list_scheduled_tasks(kind, shared, dashboard_id)
        .await
        .map_err(explain)?;

    match output {
        OutputFormat::Json => render::render_json(&tasks)?,
        OutputFormat::Toon => render::render_toon(&tasks)?,
        OutputFormat::Text => {
            if tasks.is_empty() {
                render::print_no_results("No scheduled tasks found.");
                return Ok(());
            }
            let rows = tasks
                .iter()
                .map(|task| parse_task(task).map(|task| task_row(&task)))
                .collect::<Result<Vec<_>>>()?;
            render::render_table(
                &[
                    "ID", "TITLE", "KIND", "SCHEDULE", "ENABLED", "NEXT RUN", "OWNER",
                ],
                rows,
                false,
            );
        }
    }
    Ok(())
}

fn task_row(task: &ScheduledTask) -> Vec<String> {
    let owner = task.owner.as_ref().and_then(|owner| {
        owner
            .email
            .as_deref()
            .or(owner.entity_id.as_deref())
            .map(str::to_string)
    });
    vec![
        // `render_table` expects the profile name first and drops it for a
        // single profile.
        String::new(),
        task.id.clone(),
        task.title.clone(),
        or_dash(&task.kind).to_string(),
        schedule_label(task.interval_seconds, task.interval_months),
        render::bool_display(Some(task.enabled)),
        or_dash(&task.next_run_at).to_string(),
        or_dash(&owner).to_string(),
    ]
}

/// Show one task.
pub async fn run_tasks_get(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
) -> Result<()> {
    let api = tasks_api(targets)?;
    eprintln!("{}", "Fetching scheduled task...".dimmed());
    let task = api.get_scheduled_task(task_id).await.map_err(explain)?;

    if matches!(output, OutputFormat::Json) {
        return render::render_json_auto(&[task]);
    }
    // A prompt can be 100 KiB, which is more than an agent should get inline.
    let rows = [task];
    if let SpillOutcome::Spilled { path, .. } = maybe_spill(&rows, max_direct, temp_dir)? {
        println!("Task is too large to print. Written to: {}", path.display());
        return Ok(());
    }
    let [task] = rows;
    if matches!(output, OutputFormat::Toon) {
        return render::render_toon(&[task]);
    }
    print!("{}", format_task(&parse_task(&task)?));
    Ok(())
}

/// One task as text: a line per fact, then the prompt.
fn format_task(task: &ScheduledTask) -> String {
    let mut lines = vec![
        ("ID", task.id.clone()),
        ("Title", task.title.clone()),
        ("Kind", or_dash(&task.kind).to_string()),
    ];
    if let Some(url) = task.metadata.as_ref().and_then(|m| m.dashboard_url.clone()) {
        lines.push(("Dashboard", url));
    }
    lines.extend([
        (
            "Schedule",
            format!(
                "{}, starting {}",
                schedule_label(task.interval_seconds, task.interval_months),
                task.start_at
            ),
        ),
        ("Enabled", render::bool_display(Some(task.enabled))),
        ("Next run", or_dash(&task.next_run_at).to_string()),
        ("Last run", or_dash(&task.last_run_at).to_string()),
        ("Model", or_dash(&task.model_choice).to_string()),
        ("Reasoning", or_dash(&task.reasoning_effort).to_string()),
        (
            "Notification",
            match &task.notification {
                None => "none".to_string(),
                Some(notification) => {
                    let condition = notification
                        .condition
                        .as_ref()
                        .map(|condition| format!(" \"{condition}\""))
                        .unwrap_or_default();
                    format!(
                        "{}{condition} to connectors {}",
                        notification.notification_type,
                        notification.connector_ids.join(", ")
                    )
                }
            },
        ),
        (
            "Shared with",
            task.shared_options
                .as_ref()
                .and_then(|shared| shared.shared_type.as_deref())
                .map_or("-", with_value)
                .to_string(),
        ),
    ]);
    if let Some(owner) = &task.owner {
        let name = owner.email.as_ref().or(owner.entity_id.as_ref());
        let you = if task.is_owner == Some(true) {
            " (you)"
        } else {
            ""
        };
        lines.push(("Owner", format!("{}{you}", or_dash(&name.cloned()))));
    }

    let mut out = String::new();
    for (label, value) in lines {
        out.push_str(&format!("{} {value}\n", format!("{label}:").dimmed()));
    }
    out.push_str(&format!("\n{}\n{}\n", "Prompt:".dimmed(), task.prompt));
    out
}

/// Create a task from a JSON file (the API's create body as is) or from flags.
/// `confirm` asks the user; it runs after the input is checked, so a wrong
/// file fails before the prompt.
pub async fn run_tasks_create(
    targets: &[Arc<ExecutionTarget>],
    from_file: Option<&str>,
    content: TaskContentFlags,
    schedule: Option<ScheduleFlags>,
    confirm: impl FnOnce(&str) -> Result<()>,
    output: OutputFormat,
) -> Result<()> {
    let api = tasks_api(targets)?;
    let body = match (from_file, schedule) {
        (Some(path), _) => {
            let body = read_from_file(path)?;
            let ignored = ignored_keys(
                &body,
                &[&UPDATE_FIELDS, &CREATE_ONLY_FIELDS],
                &READ_ONLY_FIELDS,
            );
            reject_ignored_keys(&ignored, "")?;
            confirm(&format!("Create scheduled task from '{path}'?"))?;
            body
        }
        (None, Some(schedule)) => {
            confirm(&format!(
                "Create scheduled task '{}'?",
                content.title.as_deref().unwrap_or_default()
            ))?;
            let mut body = serde_json::to_value(schedule.into_schedule(true)?)?;
            for (key, value) in content.into_body() {
                body[key] = value;
            }
            body
        }
        // The flags are required without --from-file, so there is a schedule.
        (None, None) => bail!("Pass --from-file, or --title, --prompt, --model and --start-at."),
    };

    eprintln!("{}", "Creating scheduled task...".dimmed());
    let created = api
        .create_scheduled_task(&body)
        .await
        .map_err(explain_create)?;
    let task = parse_task(&created)?;
    let paused = if task.enabled { "" } else { DISABLED_NOTE };
    eprintln!(
        "{}",
        format!(
            "Created scheduled task {} ({}). Next run: {}.{paused}",
            task.id,
            schedule_label(task.interval_seconds, task.interval_months),
            or_dash(&task.next_run_at)
        )
        .green()
    );
    print_written_task(created, output)
}

/// Change a task's title, prompt, model, reasoning effort or notification.
pub async fn run_tasks_update(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    from_file: Option<&str>,
    content: TaskContentFlags,
    confirm: impl FnOnce(&str) -> Result<()>,
    output: OutputFormat,
) -> Result<()> {
    let api = tasks_api(targets)?;
    let body = match from_file {
        Some(path) => {
            let body = read_from_file(path)?;
            reject_ignored_keys(
                &ignored_keys(&body, &[&UPDATE_FIELDS], &[]),
                &format!(
                    " `update` reads only {}. Use `set-schedule` for the schedule and \
                     `share` for sharing.",
                    UPDATE_FIELDS.join(", ")
                ),
            )?;
            if body.as_object().is_some_and(Map::is_empty) {
                bail!("The file is an empty object, so there is nothing to change.");
            }
            body
        }
        None => Value::Object(content.into_body()),
    };
    confirm(&format!("Update scheduled task '{task_id}'?"))?;
    owned_task(&api, task_id).await?;
    let updated = api
        .update_scheduled_task(task_id, &body)
        .await
        .map_err(explain_write)?;
    eprintln!("{}", format!("Updated scheduled task {task_id}.").green());
    print_written_task(updated, output)
}

/// Change when a task runs. A paused task stays paused: the API would turn
/// it back on unless `enabled` is sent, so the current value is read first.
pub async fn run_tasks_set_schedule(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    schedule: ScheduleFlags,
    output: OutputFormat,
) -> Result<()> {
    let api = tasks_api(targets)?;
    let (_, current) = owned_task(&api, task_id).await?;
    let schedule = schedule.into_schedule(current.enabled)?;
    let updated = api
        .set_scheduled_task_schedule(task_id, &schedule)
        .await
        .map_err(explain_write)?;
    let task = parse_task(&updated)?;
    let paused = if task.enabled { "" } else { DISABLED_NOTE };
    eprintln!(
        "{}",
        format!(
            "Scheduled task {task_id} now runs {}. Next run: {}.{paused}",
            schedule_label(task.interval_seconds, task.interval_months),
            or_dash(&task.next_run_at)
        )
        .green()
    );
    print_written_task(updated, output)
}

/// Turn a task's schedule on or off. The API has no endpoint for this alone,
/// so the task's own schedule is written back with `enabled` changed.
pub async fn run_tasks_set_enabled(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    enabled: bool,
    output: OutputFormat,
) -> Result<()> {
    let api = tasks_api(targets)?;
    let state = if enabled { "enabled" } else { "disabled" };
    let (current, task) = owned_task(&api, task_id).await?;
    if task.enabled == enabled {
        eprintln!(
            "{}",
            format!("Scheduled task {task_id} is already {state}.").yellow()
        );
        return print_written_task(current, output);
    }
    // The API refuses any schedule with a past start time and no interval,
    // whatever `enabled` is, and its message does not say what to do.
    let one_off = task.interval_seconds.is_none() && task.interval_months.is_none();
    let started = task
        .start_at
        .parse::<DateTime<Utc>>()
        .is_ok_and(|start_at| start_at <= Utc::now());
    if one_off && started {
        bail!(
            "Scheduled task {task_id} is a one-off and its start time ({}) has passed, \
             so it cannot be {state}. Give it a new time first:\n  \
             cx olly scheduled-tasks set-schedule {task_id} --start-at <time>",
            task.start_at
        );
    }

    let schedule = TaskSchedule {
        enabled,
        ..task.schedule()
    };
    let updated = api
        .set_scheduled_task_schedule(task_id, &schedule)
        .await
        .map_err(explain_write)?;
    let next_run = if enabled {
        format!(
            " Next run: {}.",
            or_dash(&parse_task(&updated)?.next_run_at)
        )
    } else {
        String::new()
    };
    eprintln!(
        "{}",
        format!("Scheduled task {task_id} is now {state}.{next_run}").green()
    );
    print_written_task(updated, output)
}

/// Change who can see a task. `with` is the CLI value: `private`, `org` or
/// `entities`.
pub async fn run_tasks_share(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    with: &str,
    entity_ids: &[String],
    output: OutputFormat,
) -> Result<()> {
    let api = tasks_api(targets)?;
    owned_task(&api, task_id).await?;
    let updated = api
        .share_scheduled_task(task_id, api_shared_type(with)?, entity_ids)
        .await
        .map_err(explain_write)?;
    eprintln!(
        "{}",
        format!("Scheduled task {task_id} is now shared with: {with}.").green()
    );
    print_written_task(updated, output)
}

/// Delete a task.
pub async fn run_tasks_delete(targets: &[Arc<ExecutionTarget>], task_id: &str) -> Result<()> {
    let api = tasks_api(targets)?;
    owned_task(&api, task_id).await?;
    api.delete_scheduled_task(task_id)
        .await
        .map_err(explain_write)?;
    eprintln!("{}", format!("Deleted scheduled task {task_id}.").green());
    Ok(())
}

/// Run a task now. The API returns nothing for a manual run, so this points
/// at `runs`, where the run and its chat appear. `profile` is the profile the
/// user chose, repeated in that command so it reaches the same team.
pub async fn run_tasks_run(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    profile: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    let api = tasks_api(targets)?;
    owned_task(&api, task_id).await?;
    api.run_scheduled_task(task_id)
        .await
        .map_err(explain_write)?;
    let profile = profile.map(|p| format!(" -p {p}")).unwrap_or_default();
    let follow = format!("cx olly scheduled-tasks runs {task_id}{profile}");
    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let row = json!({"task_id": task_id, "status": "started", "follow_with": follow});
            if matches!(output, OutputFormat::Json) {
                render::render_json_auto(&[row])
            } else {
                render::render_toon(&[row])
            }
        }
        OutputFormat::Text => {
            println!("Started a run of scheduled task {task_id}.");
            println!("{}", "Follow it with:".dimmed());
            println!("  {follow}");
            Ok(())
        }
    }
}

/// Show a task's run history, newest first.
pub async fn run_tasks_runs(
    targets: &[Arc<ExecutionTarget>],
    task_id: &str,
    output: OutputFormat,
) -> Result<()> {
    let api = tasks_api(targets)?;
    eprintln!("{}", "Fetching runs...".dimmed());
    let runs = api
        .list_scheduled_task_runs(task_id)
        .await
        .map_err(explain)?;

    match output {
        OutputFormat::Json => render::render_json(&runs)?,
        OutputFormat::Toon => render::render_toon(&runs)?,
        OutputFormat::Text => {
            if runs.is_empty() {
                render::print_no_results("This task has not run yet.");
                return Ok(());
            }
            let rows = runs
                .iter()
                .map(|run| {
                    let run: ScheduledTaskRun = serde_json::from_value(run.clone())?;
                    Ok(vec![
                        String::new(),
                        or_dash(&run.started_at).to_string(),
                        run.status,
                        or_dash(&run.finished_at).to_string(),
                        or_dash(&run.error_reason).to_string(),
                        or_dash(&run.chat_id).to_string(),
                        or_dash(&run.interaction_id).to_string(),
                    ])
                })
                .collect::<Result<Vec<_>>>()?;
            render::render_table(
                &[
                    "STARTED",
                    "STATUS",
                    "FINISHED",
                    "ERROR",
                    "CHAT ID",
                    "INTERACTION ID",
                ],
                rows,
                false,
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_accepts_whole_hours_from_one_hour_to_one_week() {
        assert_eq!(parse_every("1h"), Ok(3600));
        assert_eq!(parse_every("90m 30m"), Ok(7200));
        assert_eq!(parse_every("1d"), Ok(86400));
        assert_eq!(parse_every("1w"), Ok(604_800));
        assert_eq!(parse_every("7d"), Ok(604_800));
    }

    #[test]
    fn every_rejects_what_the_api_would_reject() {
        for (input, message) in [
            ("59m", "between 1 hour and 1 week"),
            ("1w 1h", "between 1 hour and 1 week"),
            ("0h", "between 1 hour and 1 week"),
            ("90m", "whole number of hours"),
            ("1h 1s", "whole number of hours"),
            ("daily", "Examples"),
        ] {
            let error = parse_every(input).unwrap_err();
            assert!(error.contains(message), "{input}: {error}");
        }
    }

    #[test]
    fn schedule_label_uses_the_largest_whole_unit() {
        assert_eq!(schedule_label(Some(3600), None), "every 1h");
        assert_eq!(schedule_label(Some(36 * 3600), None), "every 36h");
        assert_eq!(schedule_label(Some(48 * 3600), None), "every 2d");
        assert_eq!(schedule_label(Some(604_800), None), "every 1w");
        assert_eq!(schedule_label(None, Some(1)), "monthly");
        assert_eq!(schedule_label(None, None), "once");
    }

    #[test]
    fn schedule_flags_become_the_api_schedule() {
        let flags = |every, monthly| ScheduleFlags {
            start_at: "2026-12-01T09:00:00Z".to_string(),
            every,
            monthly,
        };
        let json = |flags: ScheduleFlags, enabled| {
            serde_json::to_value(flags.into_schedule(enabled).unwrap()).unwrap()
        };
        assert_eq!(
            json(flags(Some(21_600), false), true),
            json!({"start_at": "2026-12-01T09:00:00.000Z", "interval_seconds": 21_600, "enabled": true})
        );
        assert_eq!(
            json(flags(None, true), false),
            json!({"start_at": "2026-12-01T09:00:00.000Z", "interval_months": 1, "enabled": false})
        );
        assert_eq!(
            json(flags(None, false), true),
            json!({"start_at": "2026-12-01T09:00:00.000Z", "enabled": true})
        );
    }

    #[test]
    fn content_flags_send_only_what_was_given() {
        let body = TaskContentFlags {
            title: Some("Daily errors".to_string()),
            model: Some("gpt-5.2".to_string()),
            ..Default::default()
        }
        .into_body();
        assert_eq!(
            Value::Object(body),
            json!({"title": "Daily errors", "model_choice": "gpt-5.2"})
        );
    }

    #[test]
    fn only_the_identity_403_gets_the_key_hint() {
        let team_key = explain(CxError::Permission(
            "A user identity is required. Team API keys are not supported for this endpoint."
                .into(),
        ))
        .to_string();
        assert!(
            team_key.contains("A user identity is required."),
            "{team_key}"
        );
        assert!(team_key.contains("Use a personal API key"), "{team_key}");

        let consent = explain_create(CxError::Permission(
            "AI features are not enabled for this team".into(),
        ))
        .to_string();
        assert_eq!(
            consent,
            "Permission denied: AI features are not enabled for this team"
        );
    }

    #[test]
    fn a_409_is_the_quota_only_on_create_and_only_when_it_is_not_a_duplicate() {
        let conflict = |message: &str| CxError::Api {
            status: 409,
            message: message.into(),
        };
        let quota = "You can have at most 20 scheduled tasks. Delete an existing task before creating a new one.";
        let text = explain_create(conflict(quota)).to_string();
        assert!(
            text.starts_with(&format!("Scheduled task quota reached: {quota}")),
            "{text}"
        );
        assert!(!explain_write(conflict(quota)).to_string().contains("quota"));

        for explain in [explain_create, explain_write] {
            let text = explain(conflict("Resource already exists")).to_string();
            assert!(text.contains("with this title already exists"), "{text}");
            assert!(!text.contains("quota"), "{text}");
        }
    }

    #[test]
    fn a_404_on_a_write_names_the_owner_rule() {
        let text = explain_write(CxError::Api {
            status: 404,
            message: "Resource not found".into(),
        })
        .to_string();
        assert!(text.contains("Resource not found"), "{text}");
        assert!(text.contains("Only the owner can"), "{text}");
    }

    #[test]
    fn shared_types_map_both_ways() {
        assert_eq!(api_shared_type("org").unwrap(), "entire_organization");
        assert!(api_shared_type("everyone").is_err());
        assert_eq!(with_value("specific_entities"), "entities");
        assert_eq!(with_value("something_new"), "something_new");
    }

    #[test]
    fn ids_cannot_change_the_request_path() {
        for id in ["task-1", "0f8fad5b-d9cb-469f-a165-70867728950e", "dash_1"] {
            assert_eq!(parse_id(id), Ok(id.to_string()));
        }
        for id in [
            "",
            "../v2/chats/x",
            "a/b",
            "a?b=c",
            "a b",
            "a#b",
            "a%2Fb",
            "..",
        ] {
            assert!(parse_id(id).is_err(), "{id:?} must be rejected");
        }
    }

    #[test]
    fn ignored_keys_are_the_ones_no_request_reads() {
        let saved_task = json!({
            "id": "task-1", "title": "T", "start_at": "x", "owner": {}, "next_run_at": null,
            "notifications": {}, "interval_second": 3600
        });
        assert_eq!(
            ignored_keys(
                &saved_task,
                &[&UPDATE_FIELDS, &CREATE_ONLY_FIELDS],
                &READ_ONLY_FIELDS
            ),
            ["interval_second", "notifications"]
        );
        assert_eq!(
            ignored_keys(
                &json!({"title": "T", "enabled": false}),
                &[&UPDATE_FIELDS],
                &[]
            ),
            ["enabled"]
        );
    }

    #[test]
    fn task_text_lists_the_facts_then_the_prompt() {
        colored::control::set_override(false);
        let task: ScheduledTask = serde_json::from_value(json!({
            "id": "task-1",
            "title": "Daily errors",
            "prompt": "Summarize new errors",
            "model_choice": "gpt-5.2",
            "reasoning_effort": "medium",
            "kind": "standard",
            "start_at": "2026-11-20T09:00:00Z",
            "interval_seconds": 86400,
            "next_run_at": "2026-11-26T09:00:00Z",
            "enabled": true,
            "notification": {"type": "on_condition", "condition": "errors > 5%", "connector_ids": ["c1", "c2"]},
            "shared_options": {"shared_type": "private"},
            "owner": {"entity_id": "user-1", "email": "owner@example.com"},
            "is_owner": true
        }))
        .unwrap();
        assert_eq!(
            format_task(&task),
            "ID: task-1\n\
             Title: Daily errors\n\
             Kind: standard\n\
             Schedule: every 1d, starting 2026-11-20T09:00:00Z\n\
             Enabled: yes\n\
             Next run: 2026-11-26T09:00:00Z\n\
             Last run: -\n\
             Model: gpt-5.2\n\
             Reasoning: medium\n\
             Notification: on_condition \"errors > 5%\" to connectors c1, c2\n\
             Shared with: private\n\
             Owner: owner@example.com (you)\n\
             \nPrompt:\nSummarize new errors\n"
        );
    }
}
