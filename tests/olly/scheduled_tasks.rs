//! `cx olly scheduled-tasks`, driven through the real binary so the flags,
//! the request and the printed output are all covered.

use std::fs;

use serde_json::{json, Value};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::olly::run_tasks_list;
use coralogix_cli::config::OutputFormat;

use super::{cli_temp_home, cli_write_profile, common, cx_olly};

const TASKS: &str = "/api/v2/olly/scheduled-tasks/";
const TASK_1: &str = "/api/v2/olly/scheduled-tasks/task-1";
const SCHEDULE_1: &str = "/api/v2/olly/scheduled-tasks/task-1/schedule";

/// A recurring task as the API returns it.
fn task(id: &str, title: &str) -> Value {
    json!({
        "id": id,
        "team_id": "42",
        "title": title,
        "prompt": "Summarize new errors",
        "model_choice": "gpt-5.2",
        "reasoning_effort": "medium",
        "start_at": "2026-11-20T09:00:00Z",
        "interval_seconds": 86400,
        "interval_months": null,
        "next_run_at": "2026-11-26T09:00:00Z",
        "last_run_at": null,
        "enabled": true,
        "notification": null,
        "kind": "standard",
        "metadata": null,
        "shared_options": {"shared_type": "private", "shared_entity_ids": null},
        "owner": {"entity_id": "user-1", "email": "owner@example.com"},
        "is_owner": true,
        "created_at": "2026-11-19T08:00:00Z",
        "updated_at": "2026-11-19T08:00:00Z",
        "scheduled_task_type": "recurring"
    })
}

fn with(mut task: Value, changes: Value) -> Value {
    for (key, value) in changes.as_object().unwrap() {
        task[key] = value.clone();
    }
    task
}

fn stdout_of(cmd: &mut assert_cmd::Command) -> String {
    let output = cmd.output().expect("cx must run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn stderr_of(cmd: &mut assert_cmd::Command) -> String {
    let output = cmd.output().expect("cx must run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stderr_of_failure(cmd: &mut assert_cmd::Command) -> String {
    let output = cmd.output().expect("cx must run");
    assert!(!output.status.success(), "expected the command to fail");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

async fn server_with_home() -> (MockServer, std::path::PathBuf) {
    let server = MockServer::start().await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());
    (server, home)
}

/// Every write reads the task first, to check that the caller owns it.
async fn mount_my_task(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(TASK_1))
        .respond_with(ResponseTemplate::new(200).set_body_json(task("task-1", "Daily errors")))
        .mount(server)
        .await;
}

fn tasks_cmd(home: &std::path::Path, args: &[&str]) -> assert_cmd::Command {
    let mut cmd = cx_olly(home);
    cmd.args(["olly", "scheduled-tasks"]).args(args);
    cmd
}

// ── list ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_asks_for_my_tasks_and_prints_the_api_response_as_json() {
    let (server, home) = server_with_home().await;
    let rows = json!([
        task("task-1", "Daily errors"),
        task("task-2", "Weekly cost")
    ]);
    Mock::given(method("GET"))
        .and(path(TASKS))
        .and(query_param("scope", "owner"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&rows))
        .expect(1)
        .mount(&server)
        .await;

    let stdout = stdout_of(&mut tasks_cmd(&home, &["list", "-o", "json"]));
    assert_eq!(serde_json::from_str::<Value>(&stdout).unwrap(), rows);
}

#[tokio::test]
async fn list_sends_kind_and_shared_and_prints_a_table() {
    let (server, home) = server_with_home().await;
    let one_off = with(
        task("task-2", "Deploy check"),
        json!({"interval_seconds": null, "enabled": false, "next_run_at": null}),
    );
    Mock::given(method("GET"))
        .and(path(TASKS))
        .and(query_param("scope", "shared"))
        .and(query_param("kind", "standard"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([task("task-1", "Daily errors"), one_off])),
        )
        .expect(1)
        .mount(&server)
        .await;

    let stdout = stdout_of(&mut tasks_cmd(
        &home,
        &["list", "--shared", "--kind", "standard"],
    ));
    for expected in [
        "ID",
        "SCHEDULE",
        "NEXT RUN",
        "OWNER",
        "task-1",
        "Daily errors",
        "every 1d",
        "2026-11-26T09:00:00Z",
        "owner@example.com",
        "Deploy check",
        "once",
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
}

#[tokio::test]
async fn list_dashboard_uses_the_dashboard_endpoint() {
    let (server, home) = server_with_home().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/olly/scheduled-tasks/for-dashboard/dash-1"))
        .and(query_param("scope", "shared"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;

    let stdout = stdout_of(&mut tasks_cmd(
        &home,
        &["list", "--dashboard", "dash-1", "--shared"],
    ));
    assert!(stdout.contains("No scheduled tasks found."), "{stdout}");
}

#[tokio::test]
async fn list_rejects_multi_profile() {
    let targets = vec![
        common::test_target("a", "http://127.0.0.1:1"),
        common::test_target("b", "http://127.0.0.1:1"),
    ];
    let err = run_tasks_list(&targets, None, false, None, OutputFormat::Json)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("multi-profile"), "{err}");
}

// ── get ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_prints_the_facts_in_text_and_the_full_response_in_json() {
    let (server, home) = server_with_home().await;
    let body = with(
        task("task-1", "Daily errors"),
        json!({"notification": {"type": "every_run", "connector_ids": ["conn-1"]}}),
    );
    Mock::given(method("GET"))
        .and(path(TASK_1))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(2)
        .mount(&server)
        .await;

    let text = stdout_of(&mut tasks_cmd(&home, &["get", "task-1"]));
    for expected in [
        "ID: task-1",
        "Schedule: every 1d, starting 2026-11-20T09:00:00Z",
        "Enabled: yes",
        "Notification: every_run to connectors conn-1",
        "Owner: owner@example.com (you)",
        "Prompt:\nSummarize new errors",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }

    let json_out = stdout_of(&mut tasks_cmd(&home, &["get", "task-1", "-o", "json"]));
    assert_eq!(serde_json::from_str::<Value>(&json_out).unwrap(), body);
}

// ── create ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_from_file_sends_the_file_as_is() {
    let (server, home) = server_with_home().await;
    // Includes fields the CLI has no flag for, and read-only fields that
    // `get -o json` prints: the body must reach the API untouched.
    let body = json!({
        "id": "old-task",
        "title": "Checkout watch",
        "prompt": "Check the dashboard",
        "model_choice": "gpt-5.2",
        "start_at": "2026-12-01T09:00:00Z",
        "interval_months": 1,
        "kind": "watch_dashboard",
        "metadata": {"dashboard_url": "https://t.app.coralogix.com/#/dashboards/abc"},
        "notification": {"type": "on_condition", "condition": "errors > 5%", "connector_ids": ["c1"]},
        "shared_options": {"shared_type": "entire_organization"}
    });
    Mock::given(method("POST"))
        .and(path(TASKS))
        .and(body_json(&body))
        .respond_with(ResponseTemplate::new(201).set_body_json(with(
            task("task-9", "Checkout watch"),
            json!({"interval_seconds": null, "interval_months": 1}),
        )))
        .expect(2)
        .mount(&server)
        .await;
    let file = home.join("task.json");
    fs::write(&file, body.to_string()).unwrap();

    let stderr = stderr_of(&mut tasks_cmd(
        &home,
        &["create", "--from-file", file.to_str().unwrap(), "--yes"],
    ));
    assert!(
        stderr.contains("Created scheduled task task-9 (monthly). Next run: 2026-11-26T09:00:00Z."),
        "{stderr}"
    );

    let from_stdin = stdout_of(
        tasks_cmd(
            &home,
            &["create", "--from-file", "-", "--yes", "-o", "json"],
        )
        .write_stdin(body.to_string()),
    );
    assert_eq!(
        serde_json::from_str::<Value>(&from_stdin).unwrap()["id"],
        "task-9"
    );
}

#[tokio::test]
async fn create_from_flags_builds_the_create_body() {
    let (server, home) = server_with_home().await;
    for body in [
        json!({
            "title": "Daily errors",
            "prompt": "Summarize new errors",
            "model_choice": "gpt-5.2",
            "reasoning_effort": "high",
            "start_at": "2026-12-01T09:00:00.000Z",
            "interval_seconds": 21600,
            "enabled": true
        }),
        json!({
            "title": "Monthly cost",
            "prompt": "Summarize spend",
            "model_choice": "gpt-5.2",
            "start_at": "2026-12-01T09:00:00.000Z",
            "interval_months": 1,
            "enabled": true
        }),
        json!({
            "title": "Deploy check",
            "prompt": "Did the deploy raise errors?",
            "model_choice": "gpt-5.2",
            "start_at": "2026-12-01T09:00:00.000Z",
            "enabled": true
        }),
    ] {
        Mock::given(method("POST"))
            .and(path(TASKS))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(201).set_body_json(task("task-1", "Created")))
            .expect(1)
            .mount(&server)
            .await;
    }

    let common = ["--model", "gpt-5.2", "--start-at", "2026-12-01T09:00:00Z"];
    for extra in [
        &[
            "--title",
            "Daily errors",
            "--prompt",
            "Summarize new errors",
            "--reasoning-effort",
            "high",
            "--every",
            "6h",
        ][..],
        &[
            "--title",
            "Monthly cost",
            "--prompt",
            "Summarize spend",
            "--monthly",
        ],
        &[
            "--title",
            "Deploy check",
            "--prompt",
            "Did the deploy raise errors?",
        ],
    ] {
        tasks_cmd(&home, &["create", "--yes"])
            .args(common)
            .args(extra)
            .assert()
            .success();
    }
}

#[tokio::test]
async fn create_accepts_a_relative_start_time() {
    let (server, home) = server_with_home().await;
    Mock::given(method("POST"))
        .and(path(TASKS))
        .respond_with(ResponseTemplate::new(201).set_body_json(task("task-1", "Soon")))
        .expect(1)
        .mount(&server)
        .await;

    tasks_cmd(
        &home,
        &[
            "create",
            "--title",
            "Soon",
            "--prompt",
            "Check",
            "--model",
            "gpt-5.2",
            "--start-at",
            "now+1h",
            "--yes",
        ],
    )
    .assert()
    .success();

    let sent: Value = server.received_requests().await.unwrap()[0]
        .body_json()
        .unwrap();
    let start_at =
        chrono::DateTime::parse_from_rfc3339(sent["start_at"].as_str().unwrap()).unwrap();
    let ahead = (start_at.to_utc() - chrono::Utc::now()).num_seconds();
    assert!((3500..=3600).contains(&ahead), "start_at is {ahead}s ahead");
}

#[tokio::test]
async fn create_explains_each_refusal_with_the_api_message() {
    // The bodies are the ones the API sends (cx-olly exception handlers).
    for (status, detail, expected, unexpected) in [
        (
            409,
            "You can have at most 20 scheduled tasks. Delete an existing task before creating a new one.",
            "Scheduled task quota reached: You can have at most 20 scheduled tasks.",
            "already exists",
        ),
        (
            409,
            "Resource already exists",
            "A scheduled task with this title already exists, or one already watches this dashboard.",
            "quota",
        ),
        (
            403,
            "A user identity is required. Team API keys are not supported for this endpoint.",
            "Use a personal API key or a logged-in profile.",
            "quota",
        ),
        (
            403,
            "AI features are not enabled for this team",
            "Permission denied: AI features are not enabled for this team",
            "personal API key",
        ),
    ] {
        let (server, home) = server_with_home().await;
        Mock::given(method("POST"))
            .and(path(TASKS))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"detail": detail})))
            .expect(1)
            .mount(&server)
            .await;
        let file = home.join("task.json");
        fs::write(&file, "{}").unwrap();

        let stderr = stderr_of_failure(&mut tasks_cmd(
            &home,
            &["create", "--from-file", file.to_str().unwrap(), "--yes"],
        ));
        assert!(stderr.contains(detail), "{detail}: {stderr}");
        assert!(stderr.contains(expected), "{detail}: {stderr}");
        assert!(!stderr.contains(unexpected), "{detail}: {stderr}");
    }
}

#[tokio::test]
async fn create_fails_on_file_keys_the_api_ignores() {
    let (server, home) = server_with_home().await;
    Mock::given(method("POST"))
        .and(path(TASKS))
        .respond_with(ResponseTemplate::new(201).set_body_json(with(
            task("task-1", "Typo"),
            json!({"enabled": false, "next_run_at": null}),
        )))
        .expect(1)
        .mount(&server)
        .await;

    // A task saved with `get -o json` has read-only keys; those are expected.
    let saved = home.join("saved.json");
    fs::write(&saved, task("task-1", "Typo").to_string()).unwrap();
    let quiet = stderr_of(&mut tasks_cmd(
        &home,
        &["create", "--from-file", saved.to_str().unwrap(), "--yes"],
    ));
    assert!(quiet.contains("The task is disabled"), "{quiet}");

    let typo = home.join("typo.json");
    fs::write(
        &typo,
        json!({"title": "Typo", "interval_second": 3600, "notifications": {}}).to_string(),
    )
    .unwrap();
    let refused = stderr_of_failure(&mut tasks_cmd(
        &home,
        &["create", "--from-file", typo.to_str().unwrap(), "--yes"],
    ));
    assert!(
        refused.contains("keys this command does not read: interval_second, notifications."),
        "{refused}"
    );
}

#[tokio::test]
async fn create_checks_its_input_before_any_request() {
    let (server, home) = server_with_home().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(201))
        .expect(0)
        .mount(&server)
        .await;
    let list_file = home.join("list.json");
    fs::write(&list_file, "[1]").unwrap();
    let flags = [
        "--title", "T", "--prompt", "P", "--model", "gpt-5.2", "--yes",
    ];

    for (extra, message) in [
        (
            &["--start-at", "now+1h", "--every", "90m"][..],
            "whole number of hours",
        ),
        (
            &["--start-at", "now+1h", "--every", "2w"],
            "between 1 hour and 1 week",
        ),
        (
            &["--start-at", "now+1h", "--every", "1d", "--monthly"],
            "cannot be used with",
        ),
        (&["--start-at", "tomorrow"], "ISO-8601 time with a timezone"),
        (
            &["--start-at", "2026-12-01T09:00:00"],
            "ISO-8601 time with a timezone",
        ),
        (
            &["--start-at", "now+1h", "--reasoning-effort", "max"],
            "possible values",
        ),
        (
            &["--start-at", "now+1h", "--from-file", "x.json"],
            "cannot be used with",
        ),
        (&[], "--start-at"),
    ] {
        let stderr = stderr_of_failure(tasks_cmd(&home, &["create"]).args(flags).args(extra));
        assert!(stderr.contains(message), "{extra:?}: {stderr}");
    }

    let not_object = stderr_of_failure(&mut tasks_cmd(
        &home,
        &[
            "create",
            "--from-file",
            list_file.to_str().unwrap(),
            "--yes",
        ],
    ));
    assert!(not_object.contains("one JSON object"), "{not_object}");

    // The file is read before the prompt, so a wrong path fails first.
    let missing = stderr_of_failure(
        tasks_cmd(&home, &["create", "--from-file", "no-such-file.json"]).env("CX_AGENT_MODE", "1"),
    );
    assert!(!missing.contains("requires user confirmation"), "{missing}");

    let good_file = home.join("good.json");
    fs::write(&good_file, task("task-1", "Good").to_string()).unwrap();
    let unconfirmed = stderr_of_failure(
        tasks_cmd(
            &home,
            &["create", "--from-file", good_file.to_str().unwrap()],
        )
        .env("CX_AGENT_MODE", "1"),
    );
    assert!(
        unconfirmed.contains("requires user confirmation"),
        "{unconfirmed}"
    );
}

#[tokio::test]
async fn a_team_api_key_fails_before_any_request() {
    let (server, home) = server_with_home().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    for args in [&["list"][..], &["delete", "task-1", "--yes"]] {
        let stderr = stderr_of_failure(tasks_cmd(&home, args).args(["--api-key", "cxtp_team_key"]));
        assert!(stderr.contains("team API key"), "{args:?}: {stderr}");
        assert!(
            stderr.contains("Use a personal API key or a logged-in profile."),
            "{args:?}: {stderr}"
        );
    }
}

// ── update ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn update_sends_only_the_given_fields() {
    let (server, home) = server_with_home().await;
    mount_my_task(&server).await;
    let stop_notifications = json!({"notification": null});
    for body in [
        json!({"title": "New title", "model_choice": "claude-sonnet-4-5", "reasoning_effort": "low"}),
        json!({"prompt": "New prompt"}),
        stop_notifications.clone(),
    ] {
        Mock::given(method("PATCH"))
            .and(path(TASK_1))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(task("task-1", "New title")))
            .expect(1)
            .mount(&server)
            .await;
    }
    let file = home.join("changes.json");
    fs::write(&file, stop_notifications.to_string()).unwrap();

    for extra in [
        &[
            "--title",
            "New title",
            "--model",
            "claude-sonnet-4-5",
            "--reasoning-effort",
            "low",
        ][..],
        &["--prompt", "New prompt"],
        &["--from-file", file.to_str().unwrap()],
    ] {
        let stderr = stderr_of(tasks_cmd(&home, &["update", "task-1", "--yes"]).args(extra));
        assert!(
            stderr.contains("Updated scheduled task task-1."),
            "{stderr}"
        );
    }
}

#[tokio::test]
async fn update_needs_something_to_change() {
    let (server, home) = server_with_home().await;
    Mock::given(method("PATCH"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let nothing = stderr_of_failure(&mut tasks_cmd(&home, &["update", "task-1", "--yes"]));
    assert!(nothing.contains("required arguments"), "{nothing}");

    let both = stderr_of_failure(&mut tasks_cmd(
        &home,
        &[
            "update",
            "task-1",
            "--from-file",
            "x.json",
            "--title",
            "T",
            "--yes",
        ],
    ));
    assert!(both.contains("cannot be used with"), "{both}");

    // `PATCH` ignores the schedule, so a file with it would report success
    // and change nothing.
    let file = home.join("schedule.json");
    fs::write(
        &file,
        json!({"interval_seconds": 3600, "enabled": false}).to_string(),
    )
    .unwrap();
    let ignored = stderr_of_failure(&mut tasks_cmd(
        &home,
        &[
            "update",
            "task-1",
            "--from-file",
            file.to_str().unwrap(),
            "--yes",
        ],
    ));
    assert!(
        ignored.contains("keys this command does not read: enabled, interval_seconds."),
        "{ignored}"
    );
    assert!(ignored.contains("set-schedule"), "{ignored}");

    let empty = home.join("empty.json");
    fs::write(&empty, "{}").unwrap();
    let nothing_in_file = stderr_of_failure(&mut tasks_cmd(
        &home,
        &[
            "update",
            "task-1",
            "--from-file",
            empty.to_str().unwrap(),
            "--yes",
        ],
    ));
    assert!(
        nothing_in_file.contains("nothing to change"),
        "{nothing_in_file}"
    );
}

#[tokio::test]
async fn update_explains_a_taken_title() {
    let (server, home) = server_with_home().await;
    mount_my_task(&server).await;
    Mock::given(method("PATCH"))
        .and(path(TASK_1))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(json!({"detail": "Resource already exists"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let stderr = stderr_of_failure(&mut tasks_cmd(
        &home,
        &["update", "task-1", "--title", "Taken", "--yes"],
    ));
    assert!(
        stderr.contains("with this title already exists"),
        "{stderr}"
    );
}

// ── set-schedule, enable, disable ────────────────────────────────────────────

#[tokio::test]
async fn set_schedule_replaces_the_schedule_and_keeps_a_paused_task_paused() {
    let (server, home) = server_with_home().await;
    let paused = with(task("task-1", "Daily errors"), json!({"enabled": false}));
    Mock::given(method("GET"))
        .and(path(TASK_1))
        .respond_with(ResponseTemplate::new(200).set_body_json(&paused))
        .expect(3)
        .mount(&server)
        .await;
    for body in [
        json!({"start_at": "2026-12-01T09:00:00.000Z", "interval_seconds": 21600, "enabled": false}),
        json!({"start_at": "2026-12-01T09:00:00.000Z", "interval_months": 1, "enabled": false}),
        json!({"start_at": "2026-12-01T09:00:00.000Z", "enabled": false}),
    ] {
        Mock::given(method("PUT"))
            .and(path(SCHEDULE_1))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(with(
                paused.clone(),
                json!({"interval_seconds": 21600, "next_run_at": null}),
            )))
            .expect(1)
            .mount(&server)
            .await;
    }

    for extra in [&["--every", "6h"][..], &["--monthly"], &[]] {
        let stderr = stderr_of(
            tasks_cmd(
                &home,
                &[
                    "set-schedule",
                    "task-1",
                    "--start-at",
                    "2026-12-01T09:00:00Z",
                    "--yes",
                ],
            )
            .args(extra),
        );
        assert!(
            stderr.contains("Scheduled task task-1 now runs every 6h."),
            "{stderr}"
        );
        assert!(stderr.contains("The task is disabled"), "{stderr}");
    }
}

#[tokio::test]
async fn enable_and_disable_write_the_same_schedule_back() {
    for (verb, was_enabled, now_state) in
        [("enable", false, "enabled"), ("disable", true, "disabled")]
    {
        let (server, home) = server_with_home().await;
        let current = with(
            task("task-1", "Daily errors"),
            json!({"enabled": was_enabled}),
        );
        Mock::given(method("GET"))
            .and(path(TASK_1))
            .respond_with(ResponseTemplate::new(200).set_body_json(&current))
            .expect(1)
            .mount(&server)
            .await;
        // The start time is in the past and is sent back unchanged: the API
        // accepts that for a recurring task and keeps its cadence.
        Mock::given(method("PUT"))
            .and(path(SCHEDULE_1))
            .and(body_json(json!({
                "start_at": "2026-11-20T09:00:00Z",
                "interval_seconds": 86400,
                "enabled": !was_enabled
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(with(current.clone(), json!({"enabled": !was_enabled}))),
            )
            .expect(1)
            .mount(&server)
            .await;

        let stderr = stderr_of(&mut tasks_cmd(&home, &[verb, "task-1", "--yes"]));
        assert!(
            stderr.contains(&format!("Scheduled task task-1 is now {now_state}.")),
            "{stderr}"
        );
        assert_eq!(
            stderr.contains("Next run: 2026-11-26T09:00:00Z."),
            verb == "enable",
            "{stderr}"
        );
    }
}

#[tokio::test]
async fn enable_keeps_a_monthly_cadence() {
    let (server, home) = server_with_home().await;
    Mock::given(method("GET"))
        .and(path(TASK_1))
        .respond_with(ResponseTemplate::new(200).set_body_json(with(
            task("task-1", "Monthly cost"),
            json!({"interval_seconds": null, "interval_months": 1, "enabled": false}),
        )))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(SCHEDULE_1))
        .and(body_json(json!({
            "start_at": "2026-11-20T09:00:00Z",
            "interval_months": 1,
            "enabled": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(task("task-1", "Monthly cost")))
        .expect(1)
        .mount(&server)
        .await;

    tasks_cmd(&home, &["enable", "task-1", "--yes"])
        .assert()
        .success();
}

#[tokio::test]
async fn enable_does_nothing_when_the_task_is_already_enabled() {
    let (server, home) = server_with_home().await;
    Mock::given(method("GET"))
        .and(path(TASK_1))
        .respond_with(ResponseTemplate::new(200).set_body_json(task("task-1", "Daily errors")))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let stderr = stderr_of(&mut tasks_cmd(&home, &["enable", "task-1", "--yes"]));
    assert!(stderr.contains("is already enabled"), "{stderr}");
}

#[tokio::test]
async fn enable_and_disable_refuse_a_one_off_task_in_the_past_before_any_write() {
    // The API answers this schedule with a 422 whose text does not say what
    // to do, so the CLI stops first and names the command that helps.
    for (verb, was_enabled) in [("enable", false), ("disable", true)] {
        let (server, home) = server_with_home().await;
        Mock::given(method("GET"))
            .and(path(TASK_1))
            .respond_with(ResponseTemplate::new(200).set_body_json(with(
                task("task-1", "Deploy check"),
                json!({"interval_seconds": null, "enabled": was_enabled}),
            )))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        let stderr = stderr_of_failure(&mut tasks_cmd(&home, &[verb, "task-1", "--yes"]));
        assert!(
            stderr.contains("is a one-off and its start time (2026-11-20T09:00:00Z) has passed"),
            "{stderr}"
        );
        assert!(
            stderr.contains("cx olly scheduled-tasks set-schedule task-1 --start-at <time>"),
            "{stderr}"
        );
    }
}

#[tokio::test]
async fn enable_writes_a_one_off_task_that_has_not_started() {
    let (server, home) = server_with_home().await;
    let start_at = (chrono::Utc::now() + chrono::Duration::days(2)).to_rfc3339();
    Mock::given(method("GET"))
        .and(path(TASK_1))
        .respond_with(ResponseTemplate::new(200).set_body_json(with(
            task("task-1", "Deploy check"),
            json!({"interval_seconds": null, "enabled": false, "start_at": start_at}),
        )))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(SCHEDULE_1))
        .and(body_json(json!({"start_at": start_at, "enabled": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(task("task-1", "Deploy check")))
        .expect(1)
        .mount(&server)
        .await;

    tasks_cmd(&home, &["enable", "task-1", "--yes"])
        .assert()
        .success();
}

#[tokio::test]
async fn set_schedule_shows_the_messages_of_a_validation_error() {
    let (server, home) = server_with_home().await;
    Mock::given(method("GET"))
        .and(path(TASK_1))
        .respond_with(ResponseTemplate::new(200).set_body_json(task("task-1", "Daily errors")))
        .mount(&server)
        .await;
    // FastAPI's body for a broken schema rule: a list, with the request echoed.
    Mock::given(method("PUT"))
        .and(path(SCHEDULE_1))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({"detail": [{
            "type": "value_error",
            "loc": ["body"],
            "msg": "Value error, start_at must be within one month from now",
            "input": {"start_at": "2027-06-01T09:00:00.000Z", "enabled": true},
            "ctx": {"error": {}}
        }]})))
        .expect(1)
        .mount(&server)
        .await;

    let stderr = stderr_of_failure(&mut tasks_cmd(
        &home,
        &[
            "set-schedule",
            "task-1",
            "--start-at",
            "2027-06-01T09:00:00Z",
            "--yes",
        ],
    ));
    assert!(
        stderr.contains("API request failed (422): start_at must be within one month from now"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("input"),
        "the echoed request must not be printed: {stderr}"
    );
}

#[tokio::test]
async fn writes_on_a_task_shared_with_me_name_the_owner_rule() {
    let (server, home) = server_with_home().await;
    let shared = with(
        task("task-1", "Team watch"),
        json!({"is_owner": false, "enabled": false}),
    );
    Mock::given(method("GET"))
        .and(path(TASK_1))
        .respond_with(ResponseTemplate::new(200).set_body_json(&shared))
        .mount(&server)
        .await;
    // The API would answer a non-owner like a missing task, so no write is sent.
    for write in ["PUT", "PATCH", "POST", "DELETE"] {
        Mock::given(method(write))
            .respond_with(ResponseTemplate::new(404))
            .expect(0)
            .mount(&server)
            .await;
    }

    let text = stdout_of(&mut tasks_cmd(&home, &["get", "task-1"]));
    assert!(text.contains("Owner: owner@example.com\n"), "{text}");

    for args in [
        &["enable", "task-1", "--yes"][..],
        &["disable", "task-1", "--yes"],
        &["set-schedule", "task-1", "--start-at", "now+1h", "--yes"],
        &["update", "task-1", "--title", "Mine now", "--yes"],
        &["share", "task-1", "--with", "org", "--yes"],
        &["run", "task-1", "--yes"],
        &["delete", "task-1", "--yes"],
    ] {
        let stderr = stderr_of_failure(&mut tasks_cmd(&home, args));
        assert!(
            stderr.contains("belongs to owner@example.com. Only the owner can change it."),
            "{args:?}: {stderr}"
        );
    }
}

#[tokio::test]
async fn ids_that_would_change_the_request_path_are_rejected() {
    let (server, home) = server_with_home().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    for args in [
        &["delete", "../v2/chats/chat-1", "--yes"][..],
        &["get", "task-1/schedule"],
        &["runs", "a?b"],
        &["list", "--dashboard", "../task-1"],
    ] {
        let stderr = stderr_of_failure(&mut tasks_cmd(&home, args));
        assert!(
            stderr.contains("an ID has only letters"),
            "{args:?}: {stderr}"
        );
    }
}

// ── share, delete ────────────────────────────────────────────────────────────

#[tokio::test]
async fn share_maps_with_to_the_api_shared_type() {
    let (server, home) = server_with_home().await;
    mount_my_task(&server).await;
    for body in [
        json!({"shared_type": "private"}),
        json!({"shared_type": "entire_organization"}),
        json!({"shared_type": "specific_entities", "shared_entity_ids": ["team-a", "user-b"]}),
    ] {
        Mock::given(method("PATCH"))
            .and(path("/api/v2/olly/scheduled-tasks/task-1/shared"))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(task("task-1", "Daily errors")))
            .expect(1)
            .mount(&server)
            .await;
    }

    for extra in [
        &["--with", "private"][..],
        &["--with", "org"],
        &[
            "--with",
            "entities",
            "--entity-id",
            "team-a",
            "--entity-id",
            "user-b",
        ],
    ] {
        tasks_cmd(&home, &["share", "task-1", "--yes"])
            .args(extra)
            .assert()
            .success();
    }
}

#[tokio::test]
async fn share_checks_entity_ids_before_any_request() {
    let (server, home) = server_with_home().await;
    Mock::given(method("PATCH"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let missing = stderr_of_failure(&mut tasks_cmd(
        &home,
        &["share", "task-1", "--with", "entities", "--yes"],
    ));
    assert!(missing.contains("--entity-id"), "{missing}");

    let extra = stderr_of_failure(&mut tasks_cmd(
        &home,
        &[
            "share",
            "task-1",
            "--with",
            "org",
            "--entity-id",
            "team-a",
            "--yes",
        ],
    ));
    assert!(extra.contains("only allowed with"), "{extra}");
}

#[tokio::test]
async fn delete_asks_first_and_accepts_an_empty_204() {
    let (server, home) = server_with_home().await;
    mount_my_task(&server).await;
    Mock::given(method("DELETE"))
        .and(path(TASK_1))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let unconfirmed =
        stderr_of_failure(tasks_cmd(&home, &["delete", "task-1"]).env("CX_AGENT_MODE", "1"));
    assert!(
        unconfirmed.contains("requires user confirmation"),
        "{unconfirmed}"
    );

    let stderr = stderr_of(&mut tasks_cmd(&home, &["delete", "task-1", "--yes"]));
    assert!(
        stderr.contains("Deleted scheduled task task-1."),
        "{stderr}"
    );
}

// ── run, runs ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn run_starts_a_run_and_points_at_runs() {
    let (server, home) = server_with_home().await;
    mount_my_task(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/scheduled-tasks/task-1/manual-run"))
        .respond_with(ResponseTemplate::new(204))
        .expect(3)
        .mount(&server)
        .await;

    let text = stdout_of(&mut tasks_cmd(&home, &["run", "task-1", "--yes"]));
    assert!(
        text.contains("Started a run of scheduled task task-1."),
        "{text}"
    );
    assert!(
        text.trim_end()
            .ends_with("cx olly scheduled-tasks runs task-1"),
        "{text}"
    );

    // A selected profile is repeated, so the copied command reaches the same team.
    let with_profile = stdout_of(&mut tasks_cmd(
        &home,
        &["run", "task-1", "--yes", "-p", "default"],
    ));
    assert!(
        with_profile
            .trim_end()
            .ends_with("cx olly scheduled-tasks runs task-1 -p default"),
        "{with_profile}"
    );

    let json_out = stdout_of(&mut tasks_cmd(
        &home,
        &["run", "task-1", "--yes", "-o", "json"],
    ));
    assert_eq!(
        serde_json::from_str::<Value>(&json_out).unwrap(),
        json!({
            "task_id": "task-1",
            "status": "started",
            "follow_with": "cx olly scheduled-tasks runs task-1"
        })
    );
}

#[tokio::test]
async fn runs_prints_the_history_as_a_table_and_as_json() {
    let (server, home) = server_with_home().await;
    let runs = json!([
        {
            "id": "run-2",
            "scheduled_task_id": "task-1",
            "team_id": "42",
            "status": "failed",
            "chat_id": null,
            "interaction_id": null,
            "started_at": "2026-11-25T09:00:00Z",
            "finished_at": null,
            "error_reason": "quota_exceeded",
            "notification": null,
            "notification_verdict": null,
            "notification_message": null
        },
        {
            "id": "run-1",
            "scheduled_task_id": "task-1",
            "team_id": "42",
            "status": "succeeded",
            "chat_id": "chat-9",
            "interaction_id": "int-9",
            "started_at": "2026-11-24T09:00:00Z",
            "finished_at": "2026-11-24T09:02:10Z",
            "error_reason": null,
            "notification": null,
            "notification_verdict": {"kind": "issue", "reason": "Error rate is 7%."},
            "notification_message": "Error rate is 7%."
        }
    ]);
    Mock::given(method("GET"))
        .and(path(
            "/api/v2/olly/scheduled-tasks/task-1/scheduled-task-runs",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&runs))
        .expect(2)
        .mount(&server)
        .await;

    let text = stdout_of(&mut tasks_cmd(&home, &["runs", "task-1"]));
    for expected in [
        "STARTED",
        "STATUS",
        "FINISHED",
        "ERROR",
        "CHAT ID",
        "INTERACTION ID",
        "failed",
        "quota_exceeded",
        "succeeded",
        "2026-11-24T09:02:10Z",
        "chat-9",
        "int-9",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(
        text.find("quota_exceeded").unwrap() < text.find("chat-9").unwrap(),
        "the newest run must stay first: {text}"
    );

    let json_out = stdout_of(&mut tasks_cmd(&home, &["runs", "task-1", "-o", "json"]));
    assert_eq!(serde_json::from_str::<Value>(&json_out).unwrap(), runs);
}

#[tokio::test]
async fn runs_says_when_the_task_has_not_run() {
    let (server, home) = server_with_home().await;
    Mock::given(method("GET"))
        .and(path(
            "/api/v2/olly/scheduled-tasks/task-1/scheduled-task-runs",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let stdout = stdout_of(&mut tasks_cmd(&home, &["runs", "task-1"]));
    assert!(stdout.contains("This task has not run yet."), "{stdout}");
}

// ── read-only ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn read_only_blocks_every_write_and_allows_the_reads() {
    let (server, home) = server_with_home().await;
    Mock::given(method("GET"))
        .and(path(TASKS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;

    for args in [
        &["create", "--from-file", "x.json"][..],
        &["update", "task-1", "--title", "T"],
        &["set-schedule", "task-1", "--start-at", "now+1h"],
        &["enable", "task-1"],
        &["disable", "task-1"],
        &["share", "task-1", "--with", "org"],
        &["delete", "task-1"],
        &["run", "task-1"],
    ] {
        let stderr = stderr_of_failure(
            cx_olly(&home)
                .args(["--read-only", "olly", "scheduled-tasks"])
                .args(args)
                .arg("--yes"),
        );
        assert!(
            stderr.contains("blocked in read-only mode"),
            "{args:?}: {stderr}"
        );
    }

    cx_olly(&home)
        .args(["--read-only", "olly", "scheduled-tasks", "list"])
        .assert()
        .success();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
