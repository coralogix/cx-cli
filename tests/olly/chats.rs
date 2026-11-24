//! `cx olly chats list|get` and `cx olly models`, driven through the real
//! binary so the flags, the request and the printed output are all covered.

use std::fs;

use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
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
