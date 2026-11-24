#[path = "../common/mod.rs"]
mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use assert_cmd::Command as AssertCommand;
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::olly::{run_artifacts_get, run_ask, run_interactions_get};
use coralogix_cli::config::OutputFormat;

// ── CLI-level `--agent-to-agent-mode` flag tests ─────────────────────────────

static CLI_COUNTER: AtomicU32 = AtomicU32::new(0);

fn cli_temp_home() -> PathBuf {
    let id = CLI_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("cx_olly_cli_test_{}_{id}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn cli_write_profile(home: &std::path::Path, base_url: &str) {
    let profiles_dir = home.join(".cx").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    let content = format!(
        r#"auth = "api_key"
credential_storage = "file"
api_key = "test-key"
region = "{base_url}"
"#
    );
    fs::write(profiles_dir.join("default.toml"), content).unwrap();
    fs::write(
        home.join(".cx").join("config.toml"),
        "default_profile = \"default\"\n",
    )
    .unwrap();
}

fn cx_olly(home: &std::path::Path) -> AssertCommand {
    let mut cmd = AssertCommand::cargo_bin("cx").expect("cx binary should build");
    cmd.env("CX_HOME", home);
    cmd.env_remove("CX_API_KEY");
    cmd.env_remove("CX_REGION");
    cmd.env_remove("CX_PROFILE");
    cmd
}

#[tokio::test]
async fn artifacts_list_honors_http_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/olly/artifacts/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(2))
                .set_body_json(json!([])),
        )
        .mount(&server)
        .await;

    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let output = cx_olly(&home)
        .args(["--http-timeout", "1", "olly", "artifacts", "list"])
        .output()
        .expect("failed to run cx");

    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("HTTP request timed out"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn ask_uses_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/existing-chat/interactions/"))
        .and(body_partial_json(json!({"timeout_seconds": 42})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-1",
            "chat_id": "existing-chat",
            "status": "COMPLETED",
            "responses": []
        })))
        .mount(&server)
        .await;

    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    cx_olly(&home)
        .args([
            "olly",
            "ask",
            "What alerts fired today?",
            "--chat-id",
            "existing-chat",
            "--timeout",
            "42",
        ])
        .assert()
        .success();
}

#[tokio::test]
async fn ask_creates_chat_and_sends_message() {
    let server = MockServer::start().await;

    // Mock chat creation
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chat-123",
            "title": "",
            "user_id": "user-456",
            "created_at": "2024-01-01T00:00:00Z",
            "type": "cli"
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Mock send message with blocking response
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-123/interactions/"))
        .and(header("interaction-source", "cli"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-789",
            "chat_id": "chat-123",
            "status": "COMPLETED",
            "interaction_mode": "AGENTIC",
            "model_choice": "DEFAULT",
            "responses": [
                {
                    "id": "msg-001",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Here are the alerts from today..."}]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_ask(
        &targets,
        "What alerts fired today?",
        None,
        "gpt-5.2",
        Some(900),
        true,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_ask should succeed");
}

#[tokio::test]
async fn ask_continues_existing_chat() {
    let server = MockServer::start().await;

    // No chat creation - we're continuing an existing chat
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/existing-chat/interactions/"))
        .and(header("interaction-source", "cli"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-002",
            "chat_id": "existing-chat",
            "status": "COMPLETED",
            "responses": [
                {
                    "id": "msg-002",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Following up on your previous question..."}]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_ask(
        &targets,
        "Tell me more",
        Some("existing-chat"),
        "gpt-5.2",
        Some(900),
        true,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_ask with chat_id should succeed");
}

#[tokio::test]
async fn ask_with_different_models() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chat-deep",
            "title": ""
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-deep/interactions/"))
        .and(header("interaction-source", "cli"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-deep",
            "chat_id": "chat-deep",
            "status": "COMPLETED",
            "interaction_mode": "SKILL",
            "model_choice": "ADVANCED",
            "responses": [
                {
                    "id": "msg-deep",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Deep research results..."}]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_ask(
        &targets,
        "Analyze this complex issue",
        None,
        "advanced",
        Some(900),
        true,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_ask with advanced model should succeed");
}

#[tokio::test]
async fn ask_handles_cancelled_response() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chat-cancel",
            "title": ""
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-cancel/interactions/"))
        .and(header("interaction-source", "cli"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-cancel",
            "chat_id": "chat-cancel",
            "status": "CANCELLED",
            "responses": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_ask(
        &targets,
        "Query that gets cancelled",
        None,
        "gpt-5.2",
        Some(900),
        true,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_ask should handle cancelled status");
}

#[tokio::test]
async fn ask_rejects_multi_profile() {
    let target1 = common::test_target("profile1", "http://localhost:1");
    let target2 = common::test_target("profile2", "http://localhost:2");
    let targets = vec![target1, target2];

    let result = run_ask(
        &targets,
        "Hello",
        None,
        "gpt-5.2",
        Some(900),
        true,
        None,
        OutputFormat::Json,
    )
    .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("does not support multi-profile"));
}

#[tokio::test]
async fn ask_sends_skill_interaction_mode() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chat-skill",
            "title": ""
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-skill/interactions/"))
        .and(header("interaction-source", "cli"))
        .and(body_partial_json(json!({"interaction_mode": "skill"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-skill",
            "chat_id": "chat-skill",
            "status": "COMPLETED",
            "responses": [
                {
                    "id": "msg-skill",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Response in skill mode"}]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_ask(
        &targets,
        "Test message",
        None,
        "gpt-5.2",
        Some(900),
        true,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_ask should send skill interaction_mode");
}

#[tokio::test]
async fn ask_sends_agent_to_agent_mode_when_enabled() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chat-a2a",
            "title": ""
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-a2a/interactions/"))
        .and(header("interaction-source", "cli"))
        .and(body_partial_json(json!({"agent_to_agent_mode": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-a2a",
            "chat_id": "chat-a2a",
            "status": "COMPLETED",
            "responses": [
                {
                    "id": "msg-a2a",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Concise sub-agent response"}]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_ask(
        &targets,
        "Test message",
        None,
        "gpt-5.2",
        Some(900),
        true,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_ask should send agent_to_agent_mode: true");
}

#[tokio::test]
async fn ask_can_disable_agent_to_agent_mode() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chat-human",
            "title": ""
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-human/interactions/"))
        .and(header("interaction-source", "cli"))
        .and(body_partial_json(json!({"agent_to_agent_mode": false})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-human",
            "chat_id": "chat-human",
            "status": "COMPLETED",
            "responses": [
                {
                    "id": "msg-human",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Full human-facing response"}]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    run_ask(
        &targets,
        "Test message",
        None,
        "gpt-5.2",
        Some(900),
        false,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_ask should send agent_to_agent_mode: false");
}

#[tokio::test]
async fn artifacts_get_downloads_content() {
    let server = MockServer::start().await;

    // The presigned URL will point back to our mock server
    let presigned_path = "/mock-storage/artifact-123";
    let artifact_content = r#"{"data": "artifact content here", "rows": [1, 2, 3]}"#;

    // Mock artifact metadata endpoint
    Mock::given(method("GET"))
        .and(path("/api/v2/olly/artifacts/artifact-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "artifact-123",
            "download_url": format!("{}{}", server.uri(), presigned_path),
            "filename": "results.json",
            "content_type": "application/json",
            "size": artifact_content.len()
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Mock presigned URL endpoint (returns actual content)
    Mock::given(method("GET"))
        .and(path(presigned_path))
        .respond_with(ResponseTemplate::new(200).set_body_string(artifact_content))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let tmp_dir = std::env::temp_dir();
    let tmp_str = tmp_dir.to_str().unwrap();

    run_artifacts_get(&targets, "artifact-123", OutputFormat::Json, None, tmp_str)
        .await
        .expect("run_artifacts_get should download content");
}

#[tokio::test]
async fn artifacts_get_handles_no_download_url() {
    let server = MockServer::start().await;

    // Mock artifact without download_url
    Mock::given(method("GET"))
        .and(path("/api/v2/olly/artifacts/artifact-no-url"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "artifact-no-url",
            "filename": "missing.json",
            "content_type": "application/json"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let tmp_dir = std::env::temp_dir();
    let tmp_str = tmp_dir.to_str().unwrap();

    run_artifacts_get(
        &targets,
        "artifact-no-url",
        OutputFormat::Json,
        None,
        tmp_str,
    )
    .await
    .expect("run_artifacts_get should handle missing download_url");
}

#[tokio::test]
async fn artifacts_get_rejects_multi_profile() {
    let target1 = common::test_target("profile1", "http://localhost:1");
    let target2 = common::test_target("profile2", "http://localhost:2");
    let targets = vec![target1, target2];

    let tmp_dir = std::env::temp_dir();
    let tmp_str = tmp_dir.to_str().unwrap();

    let result =
        run_artifacts_get(&targets, "artifact-123", OutputFormat::Json, None, tmp_str).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("does not support multi-profile"));
}

#[tokio::test]
async fn agent_to_agent_mode_defaults_to_false_in_cli() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chat-cli-default",
            "title": ""
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-cli-default/interactions/"))
        .and(body_partial_json(json!({"agent_to_agent_mode": false})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-cli-default",
            "chat_id": "chat-cli-default",
            "status": "COMPLETED",
            "responses": [
                {
                    "id": "msg-cli-default",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Human-facing response"}]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    cx_olly(&home)
        .args(["olly", "ask", "Hello there"])
        .assert()
        .success();
}

#[tokio::test]
async fn agent_to_agent_mode_rejects_explicit_value() {
    let home = cli_temp_home();
    cli_write_profile(&home, "http://localhost:1");

    // Bare boolean flag - clap must reject any explicit value before any
    // network call is attempted (asserted by not mounting a mock server).
    let output = cx_olly(&home)
        .args(["olly", "ask", "Hello there", "--agent-to-agent-mode=true"])
        .output()
        .expect("failed to run cx");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected value"), "stderr: {stderr}");
}

#[tokio::test]
async fn agent_to_agent_mode_does_not_swallow_positional_message() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chat-cli-order",
            "title": ""
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/chat-cli-order/interactions/"))
        .and(body_partial_json(json!({
            "agent_to_agent_mode": true,
            "content": [{"type": "input_text", "text": "hello world"}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-cli-order",
            "chat_id": "chat-cli-order",
            "status": "COMPLETED",
            "responses": [
                {
                    "id": "msg-cli-order",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Concise response"}]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    // The flag comes *before* the positional message on the command line -
    // since it's a no-value boolean flag, it must not consume "hello world".
    cx_olly(&home)
        .args(["olly", "ask", "--agent-to-agent-mode", "hello world"])
        .assert()
        .success();
}

// ── `ask --no-wait` and `interactions get` ───────────────────────────────────

#[tokio::test]
async fn ask_no_wait_does_not_block_and_prints_the_next_command() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/existing-chat/interactions/"))
        .and(body_partial_json(json!({"should_block": false})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-1",
            "chat_id": "existing-chat",
            "status": "in_progress",
            "responses": null
        })))
        .expect(2)
        .mount(&server)
        .await;

    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let output = cx_olly(&home)
        .env("NO_COLOR", "1")
        .args([
            "olly",
            "ask",
            "Investigate the checkout latency",
            "--chat-id",
            "existing-chat",
            "--no-wait",
            "-o",
            "text",
        ])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success(), "{output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Status: in_progress"), "stdout: {stdout}");
    assert!(
        stdout
            .trim_end()
            .ends_with("cx olly interactions get existing-chat interaction-1"),
        "stdout: {stdout}"
    );

    // A selected profile is repeated, so the copied command reaches the same team.
    let output = cx_olly(&home)
        .env("NO_COLOR", "1")
        .args(["-p", "default", "olly", "ask", "Again", "--no-wait"])
        .args(["--chat-id", "existing-chat", "-o", "text"])
        .output()
        .expect("failed to run cx");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout
            .trim_end()
            .ends_with("cx olly interactions get existing-chat interaction-1 -p default"),
        "stdout: {stdout}"
    );

    // The timeout only applies to blocking requests, so it is not sent.
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(body.get("timeout_seconds").is_none(), "body: {body}");
}

#[tokio::test]
async fn blocking_ask_that_is_still_in_progress_points_at_interactions_get() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/existing-chat/interactions/"))
        .and(body_partial_json(json!({"should_block": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-1",
            "chat_id": "existing-chat",
            "status": "in_progress",
            "responses": null
        })))
        .mount(&server)
        .await;

    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let output = cx_olly(&home)
        .env("NO_COLOR", "1")
        .args(["olly", "ask", "Investigate", "--chat-id", "existing-chat"])
        .args(["-o", "text"])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success(), "{output:?}");

    // Running `ask` again would send the question again.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout
            .trim_end()
            .ends_with("cx olly interactions get existing-chat interaction-1"),
        "stdout: {stdout}"
    );
    assert!(
        !stdout.contains("Run this command again"),
        "stdout: {stdout}"
    );
}

#[tokio::test]
async fn ask_no_wait_json_has_the_ask_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/existing-chat/interactions/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-1",
            "chat_id": "existing-chat",
            "status": "in_progress",
            "responses": null
        })))
        .mount(&server)
        .await;

    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let output = cx_olly(&home)
        .args([
            "olly",
            "ask",
            "Investigate",
            "--chat-id",
            "existing-chat",
            "--no-wait",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success(), "{output:?}");
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        v,
        json!([{
            "chat_id": "existing-chat",
            "interaction_id": "interaction-1",
            "status": "in_progress"
        }])
    );
}

#[tokio::test]
async fn ask_without_no_wait_still_blocks() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/olly/v2/chats/existing-chat/interactions/"))
        .and(body_partial_json(
            json!({"should_block": true, "timeout_seconds": 900}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-1",
            "chat_id": "existing-chat",
            "status": "completed",
            "responses": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    cx_olly(&home)
        .args(["olly", "ask", "Quick one", "--chat-id", "existing-chat"])
        .assert()
        .success();
}

#[test]
fn ask_no_wait_rejects_timeout() {
    let home = cli_temp_home();
    cli_write_profile(&home, "http://127.0.0.1:1");

    let output = cx_olly(&home)
        .args(["olly", "ask", "Investigate", "--no-wait", "--timeout", "60"])
        .output()
        .expect("failed to run cx");
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be used with"), "stderr: {stderr}");
}

/// Runs `cx olly interactions get` against a mock that answers with `status`
/// and `responses`, and returns stdout.
async fn interactions_get_stdout(
    status: &str,
    responses: serde_json::Value,
    format: &str,
) -> String {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/api/v2/olly/v2/chats/chat-1/interactions/interaction-1",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "interaction-1",
            "chat_id": "chat-1",
            "created_at": "2026-11-23T10:00:00Z",
            "status": status,
            "model_choice": "gpt-5.2",
            "reasoning_effort": null,
            "responses": responses
        })))
        .expect(1)
        .mount(&server)
        .await;

    let home = cli_temp_home();
    cli_write_profile(&home, &server.uri());

    let output = cx_olly(&home)
        .env("NO_COLOR", "1")
        .args([
            "olly",
            "interactions",
            "get",
            "chat-1",
            "interaction-1",
            "-o",
            format,
        ])
        .output()
        .expect("failed to run cx");
    assert!(output.status.success(), "{status}: {output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn answer(text: &str) -> serde_json::Value {
    json!([{
        "id": "msg-1",
        "role": "assistant",
        "content": [{"type": "text", "text": text}]
    }])
}

#[tokio::test]
async fn interactions_get_prints_the_answer_for_statuses_that_have_one() {
    for status in ["completed", "stopped", "timed_out"] {
        let stdout =
            interactions_get_stdout(status, answer("Latency rose at 14:02."), "text").await;
        assert!(
            stdout.contains(&format!("Status: {status}")),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("Latency rose at 14:02."),
            "stdout: {stdout}"
        );
    }
}

#[tokio::test]
async fn interactions_get_says_plainly_when_there_is_no_answer() {
    let stdout = interactions_get_stdout("in_progress", json!(null), "text").await;
    assert!(stdout.contains("Status: in_progress"), "stdout: {stdout}");
    assert!(stdout.contains("No answer yet"), "stdout: {stdout}");

    let stdout = interactions_get_stdout("error", json!(null), "text").await;
    assert!(stdout.contains("Status: error"), "stdout: {stdout}");
    assert!(stdout.contains("There is no answer"), "stdout: {stdout}");
}

#[tokio::test]
async fn interactions_get_json_has_the_ask_shape() {
    let stdout = interactions_get_stdout("completed", answer("Done."), "json").await;
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        v,
        json!([{
            "chat_id": "chat-1",
            "interaction_id": "interaction-1",
            "status": "completed",
            "response": "Done.",
            "model_choice": "gpt-5.2"
        }])
    );
}

#[tokio::test]
async fn interactions_get_rejects_multi_profile() {
    let targets = vec![
        common::test_target("a", "http://127.0.0.1:1"),
        common::test_target("b", "http://127.0.0.1:1"),
    ];
    let err = run_interactions_get(&targets, "chat-1", "interaction-1", OutputFormat::Json)
        .await
        .expect_err("multi-profile must be rejected");
    assert!(err.to_string().contains("multi-profile"), "{err}");
}
