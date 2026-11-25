use serde_json::json;
use wiremock::matchers::{body_json, body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::olly::scheduled_tasks::{
    run_create, run_delete, run_list, run_now, run_runs, run_set_enabled, run_share, run_update,
    Cadence, CreateTaskArgs, UpdateTaskArgs,
};
use coralogix_cli::config::OutputFormat;

use crate::common;

const BASE: &str = "/api/v2/olly/scheduled-tasks";
const TASK_ID: &str = "11111111-2222-4333-8444-555555555555";

fn task(extra: serde_json::Value) -> serde_json::Value {
    let mut t = json!({
        "id": TASK_ID,
        "title": "Checkout errors",
        "prompt": "Check checkout errors",
        "model_choice": "gpt-5.2",
        "start_at": "2026-11-01T09:00:00Z",
        "interval_seconds": 86400,
        "interval_months": null,
        "enabled": true,
        "shared_options": { "shared_type": "private" },
        "owner": { "entity_id": "u1", "email": "owner@example.com" },
        "is_owner": true
    });
    for (k, v) in extra.as_object().unwrap() {
        t[k] = v.clone();
    }
    t
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

#[tokio::test]
async fn list_asks_for_shared_tasks_with_the_shared_scope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/")))
        .and(query_param("scope", "shared"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([task(json!({}))])))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("p", &server.uri())];
    run_list(&targets, true, OutputFormat::Text)
        .await
        .expect("run_list should succeed");
}

#[tokio::test]
async fn create_posts_the_schedule_and_notification() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/")))
        .and(body_partial_json(json!({
            "title": "Checkout errors",
            "prompt": "Check checkout errors",
            "model_choice": "gpt-5.2",
            "start_at": "2026-12-01T09:00:00Z",
            "interval_seconds": 21600,
            "interval_months": null,
            "enabled": true,
            "shared_options": { "shared_type": "entire_organization" },
            "notification": { "type": "olly_judges", "connector_ids": ["c1"] }
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(task(json!({}))))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("p", &server.uri())];
    let connectors = vec!["c1".to_string()];
    run_create(
        &targets,
        &CreateTaskArgs {
            title: "Checkout errors",
            prompt: "Check checkout errors",
            model: "gpt-5.2",
            reasoning_effort: None,
            start_at: "2026-12-01T09:00:00Z",
            cadence: Cadence::EverySeconds(21600),
            disabled: false,
            notify: Some("olly-judges"),
            connector_ids: &connectors,
            condition: None,
            share_with_team: true,
        },
        OutputFormat::Json,
    )
    .await
    .expect("run_create should succeed");
}

#[tokio::test]
async fn create_rejects_bad_input_without_calling_the_api() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(201))
        .expect(0)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("p", &server.uri())];
    let err = run_create(
        &targets,
        &CreateTaskArgs {
            title: "t",
            prompt: "p",
            model: "gpt-5.2",
            reasoning_effort: None,
            start_at: "now+1h",
            cadence: Cadence::Once,
            disabled: false,
            notify: Some("on-condition"),
            connector_ids: &["c1".to_string()],
            condition: None,
            share_with_team: false,
        },
        OutputFormat::Json,
    )
    .await
    .expect_err("on-condition without a condition must fail");
    assert!(err.to_string().contains("--condition"));
}

#[tokio::test]
async fn update_patches_content_then_replaces_the_schedule() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("{BASE}/{TASK_ID}")))
        .and(body_json(json!({ "title": "Renamed" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(task(json!({ "title": "Renamed" }))))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("{BASE}/{TASK_ID}/schedule")))
        .and(body_json(json!({
            "start_at": "2026-11-01T09:00:00Z",
            "interval_seconds": null,
            "interval_months": 1,
            "enabled": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(task(json!({}))))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("p", &server.uri())];
    let mut args = update_args();
    args.title = Some("Renamed");
    args.cadence = Some(Cadence::Monthly);
    run_update(&targets, TASK_ID, &args, OutputFormat::Json)
        .await
        .expect("run_update should succeed");
}

#[tokio::test]
async fn disable_keeps_the_schedule_and_turns_enabled_off() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/{TASK_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(task(json!({}))))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("{BASE}/{TASK_ID}/schedule")))
        .and(body_json(json!({
            "start_at": "2026-11-01T09:00:00Z",
            "interval_seconds": 86400,
            "interval_months": null,
            "enabled": false
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(task(json!({ "enabled": false }))))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("p", &server.uri())];
    run_set_enabled(&targets, TASK_ID, false, OutputFormat::Json)
        .await
        .expect("run_set_enabled should succeed");
}

#[tokio::test]
async fn share_with_the_team_patches_shared_options() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("{BASE}/{TASK_ID}/shared")))
        .and(body_json(json!({ "shared_type": "entire_organization" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(task(json!({
            "shared_options": { "shared_type": "entire_organization" }
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("p", &server.uri())];
    run_share(&targets, TASK_ID, true, OutputFormat::Json)
        .await
        .expect("run_share should succeed");
}

#[tokio::test]
async fn delete_and_run_accept_no_content() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("{BASE}/{TASK_ID}")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/{TASK_ID}/manual-run")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("p", &server.uri())];
    run_delete(&targets, TASK_ID, OutputFormat::Json)
        .await
        .expect("run_delete should succeed");
    run_now(&targets, TASK_ID, OutputFormat::Json)
        .await
        .expect("run_now should succeed");
}

#[tokio::test]
async fn runs_lists_the_history() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/{TASK_ID}/scheduled-task-runs")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": "r1",
            "scheduled_task_id": TASK_ID,
            "status": "succeeded",
            "started_at": "2026-11-20T09:00:00Z",
            "finished_at": "2026-11-20T09:02:00Z",
            "chat_id": "c1",
            "notification_verdict": { "kind": "issue", "reason": "Error rate 7%, above 5%." }
        }])))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("p", &server.uri())];
    run_runs(&targets, TASK_ID, OutputFormat::Text)
        .await
        .expect("run_runs should succeed");
}

#[tokio::test]
async fn a_non_uuid_task_id_is_rejected_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("p", &server.uri())];
    let err = run_delete(&targets, "", OutputFormat::Json)
        .await
        .expect_err("an empty ID must be rejected");
    assert!(err.to_string().contains("expected a UUID"));
}
