//! `cx olly chats` and `cx olly models`, driven through the real
//! binary so the flags, the request and the printed output are all covered.

use std::fs;

use serde_json::{json, Value};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::olly::run_chats_list;
use coralogix_cli::config::OutputFormat;

use super::{cli_temp_home, cli_write_profile, common, cx_olly};

const CHATS: &str = "/api/v2/olly/v2/chats/";

fn chat_row(id: &str, title: &str) -> Value {
    json!({
        "id": id,
        "title": title,
        "entity_id": "user-1",
        "created_at": "2026-11-24T11:54:16.594717Z",
        "shared_options": {"shared_type": "private", "shared_entity_ids": null},
        "task_shared_options": null,
        "type": "ai_connector",
        "metadata": null,
        "execution_runtime": "default"
    })
}

fn chat_with_messages() -> Value {
    let mut chat = chat_row("chat-1", "Checkout latency");
    chat["messages"] = json!([
        {"id": "m1", "role": "user", "content": [{"type": "text", "text": "Why is checkout slow?"}]},
        {"id": "m2", "role": "assistant", "content": [{"type": "text", "text": "A deploy at 10:02 added a slow query."}]}
    ]);
    chat["interactions"] = json!([{"id": "i1", "chat_id": "chat-1", "status": "COMPLETED"}]);
    chat
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

#[tokio::test]
async fn chats_list_defaults_to_50_and_prints_the_api_response_as_json() {
    let server = MockServer::start().await;
    let rows = json!([chat_row("chat-1", "First"), chat_row("chat-2", "Second")]);
    Mock::given(method("GET"))
        .and(path(CHATS))
        .and(query_param("limit", "50"))
        .and(query_param("skip", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&rows))
        .expect(1)
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let stdout = stdout_of(cx_olly(&home).args(["olly", "chats", "list", "-o", "json"]));
    assert_eq!(serde_json::from_str::<Value>(&stdout).unwrap(), rows);
}

#[tokio::test]
async fn chats_list_sends_filters_and_paging_and_prints_a_table() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(CHATS))
        .and(query_param("limit", "10"))
        .and(query_param("skip", "20"))
        .and(query_param("chat_type", "scheduled"))
        .and(query_param("metadata", r#"{"scheduled_task_id":"task-1"}"#))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([chat_row("chat-1", "Daily report")])),
        )
        .expect(1)
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let stdout = stdout_of(cx_olly(&home).args([
        "olly",
        "chats",
        "list",
        "--type",
        "scheduled",
        "--metadata",
        r#"{"scheduled_task_id":"task-1"}"#,
        "--limit",
        "10",
        "--offset",
        "20",
    ]));
    for expected in ["ID", "TITLE", "chat-1", "Daily report", "ai_connector"] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
}

#[tokio::test]
async fn chats_list_rejects_bad_flags_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(0)
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    for (flag, value, message) in [
        ("--metadata", "{not json", "invalid JSON"),
        ("--metadata", "[1]", "must be a JSON object"),
        ("--type", "email", "possible values"),
        ("--limit", "0", "1..=10000"),
    ] {
        let output = cx_olly(&home)
            .args(["olly", "chats", "list", flag, value])
            .output()
            .expect("cx must run");
        assert!(!output.status.success(), "{flag} {value} must fail");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{flag} {value}: {stderr}");
    }
}

#[tokio::test]
async fn chats_list_rejects_multi_profile() {
    let targets = vec![
        common::test_target("a", "http://127.0.0.1:1"),
        common::test_target("b", "http://127.0.0.1:1"),
    ];
    let err = run_chats_list(&targets, None, None, 50, 0, OutputFormat::Json)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("multi-profile"), "{err}");
}

#[tokio::test]
async fn chats_get_prints_the_conversation_in_text_and_the_full_response_in_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/olly/v2/chats/chat-1"))
        .and(query_param("response_format", "content_blocks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_with_messages()))
        .expect(2)
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let text = stdout_of(cx_olly(&home).args(["olly", "chats", "get", "chat-1"]));
    assert_eq!(
        text,
        "Chat ID: chat-1\nTitle: Checkout latency\n\
         \nuser\nWhy is checkout slow?\n\
         \nassistant\nA deploy at 10:02 added a slow query.\n"
    );

    let json = stdout_of(cx_olly(&home).args(["olly", "chats", "get", "chat-1", "-o", "json"]));
    assert_eq!(
        serde_json::from_str::<Value>(&json).unwrap(),
        chat_with_messages()
    );
}

#[tokio::test]
async fn chats_get_spills_a_large_chat_to_a_file() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/olly/v2/chats/chat-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_with_messages()))
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());
    let spill_dir = home.join("spill");
    fs::create_dir_all(&spill_dir).unwrap();
    fs::write(
        home.join(".cx").join("config.toml"),
        format!(
            "default_profile = \"default\"\nmax_dataprime_direct_output_size = 100\ntemp_dir = {:?}\n",
            spill_dir.to_str().unwrap()
        ),
    )
    .unwrap();

    for format in ["text", "toon"] {
        let stdout =
            stdout_of(cx_olly(&home).args(["olly", "chats", "get", "chat-1", "-o", format]));
        assert!(stdout.contains("Written to:"), "{format}: {stdout}");
        assert!(!stdout.contains("slow query"), "{format}: {stdout}");
    }
    let spilled = fs::read_dir(&spill_dir).unwrap().next().unwrap().unwrap();
    let content: Value =
        serde_json::from_str(&fs::read_to_string(spilled.path()).unwrap()).unwrap();
    assert_eq!(content, json!([chat_with_messages()]));
}

#[tokio::test]
async fn chats_get_shows_the_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/olly/v2/chats/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "Chat not found"})))
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let output = cx_olly(&home)
        .args(["olly", "chats", "get", "missing"])
        .output()
        .expect("cx must run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("404"), "{stderr}");
}

#[tokio::test]
async fn models_prints_one_id_per_line_in_text_and_an_array_in_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/olly/v2/chats/models"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!(["gpt-5.2", "claude-sonnet-4-5"])),
        )
        .expect(2)
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let text = stdout_of(cx_olly(&home).args(["olly", "models"]));
    assert_eq!(text, "gpt-5.2\nclaude-sonnet-4-5\n");

    let json = stdout_of(cx_olly(&home).args(["olly", "models", "-o", "json"]));
    assert_eq!(
        serde_json::from_str::<Value>(&json).unwrap(),
        json!(["gpt-5.2", "claude-sonnet-4-5"])
    );
}

// ── Write commands ───────────────────────────────────────────────────────────

fn stderr_of_failure(cmd: &mut assert_cmd::Command) -> String {
    let output = cmd.output().expect("cx must run");
    assert!(!output.status.success(), "expected the command to fail");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[tokio::test]
async fn chats_rename_sends_the_title_and_accepts_an_empty_response() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/api/v2/olly/v2/chats/chat-1/title"))
        .and(body_json(json!({"title": "Checkout latency"})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let output = cx_olly(&home)
        .args([
            "olly",
            "chats",
            "rename",
            "chat-1",
            "Checkout latency",
            "--yes",
        ])
        .output()
        .expect("cx must run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("Renamed chat chat-1"), "{stderr}");
}

#[tokio::test]
async fn chats_delete_needs_confirmation_and_names_what_else_is_deleted() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v2/olly/v2/chats/chat-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let stderr = stderr_of_failure(
        cx_olly(&home)
            .env("CX_AGENT_MODE", "1")
            .args(["olly", "chats", "delete", "chat-1"]),
    );
    assert!(stderr.contains("requires user confirmation"), "{stderr}");
    assert!(
        stderr.contains("interactions, artifacts and uploaded files"),
        "{stderr}"
    );

    cx_olly(&home)
        .args(["olly", "chats", "delete", "chat-1", "--yes"])
        .assert()
        .success();
}

#[tokio::test]
async fn chats_share_maps_with_to_the_api_shared_type() {
    let server = MockServer::start().await;
    for body in [
        json!({"shared_type": "private"}),
        json!({"shared_type": "entire_organization"}),
        json!({"shared_type": "specific_entities", "shared_entity_ids": ["team-a", "user-b"]}),
    ] {
        Mock::given(method("PATCH"))
            .and(path("/api/v2/olly/v2/chats/chat-1/shared"))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
    }
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

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
        cx_olly(&home)
            .args(["olly", "chats", "share", "chat-1", "--yes"])
            .args(extra)
            .assert()
            .success();
    }
}

#[tokio::test]
async fn chats_share_checks_entity_ids_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let missing = stderr_of_failure(cx_olly(&home).args([
        "olly", "chats", "share", "chat-1", "--with", "entities", "--yes",
    ]));
    assert!(missing.contains("--entity-id"), "{missing}");

    for with in ["private", "org"] {
        let extra = stderr_of_failure(cx_olly(&home).args([
            "olly",
            "chats",
            "share",
            "chat-1",
            "--with",
            with,
            "--entity-id",
            "team-a",
            "--yes",
        ]));
        assert!(
            extra.contains("only allowed with `--with entities`"),
            "{with}: {extra}"
        );
    }
}

#[tokio::test]
async fn chats_stop_runs_without_confirmation_and_shows_the_api_400() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-1/stop"))
        .and(body_json(json!({"interaction_id": "running"})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-1/stop"))
        .and(body_json(json!({"interaction_id": "finished"})))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"detail": "Interaction is not in progress"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    // No --yes, and agent mode on: `stop` must still go through.
    cx_olly(&home)
        .env("CX_AGENT_MODE", "1")
        .args(["olly", "chats", "stop", "chat-1", "running"])
        .assert()
        .success();

    let stderr =
        stderr_of_failure(cx_olly(&home).args(["olly", "chats", "stop", "chat-1", "finished"]));
    assert!(stderr.contains("400"), "{stderr}");
    assert!(
        stderr.contains("Interaction is not in progress"),
        "{stderr}"
    );
}

#[tokio::test]
async fn read_only_blocks_every_chat_write() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    for args in [
        &["rename", "chat-1", "New title"][..],
        &["delete", "chat-1"],
        &["share", "chat-1", "--with", "org"],
        &["stop", "chat-1", "interaction-1"],
    ] {
        let stderr = stderr_of_failure(
            cx_olly(&home)
                .args(["--read-only", "olly", "chats"])
                .args(args)
                .arg("--yes"),
        );
        assert!(
            stderr.to_lowercase().contains("read-only"),
            "{args:?}: {stderr}"
        );
    }
}
