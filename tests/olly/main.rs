#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::olly::{run_artifacts_get, run_ask};
use coralogix_cli::config::OutputFormat;

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
        900,
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
        900,
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
        900,
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
        900,
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

    let result = run_ask(&targets, "Hello", None, "gpt-5.2", 900, OutputFormat::Json).await;

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
        900,
        OutputFormat::Json,
    )
    .await
    .expect("run_ask should send skill interaction_mode");
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
