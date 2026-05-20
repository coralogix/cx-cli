use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api_client::CxClient;
use crate::error::Result;

// ── Base paths ─────────────────────────────────────────────────────────────────

const CHATS_BASE: &str = "/api/v2/olly/v2/chats";
const ARTIFACTS_BASE: &str = "/api/v2/olly/artifacts";

// ── Request types ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct InputContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl InputContentBlock {
    pub fn text(text: &str) -> Self {
        Self {
            content_type: "input_text".to_string(),
            text: text.to_string(),
        }
    }
}

// ── Response types ─────────────────────────────────────────────────────────────
// Note: The Olly API uses snake_case for JSON fields (not camelCase like other CX APIs)

#[derive(Debug, Deserialize)]
pub struct SharedOptions {
    pub shared_type: Option<String>,
    pub shared_user_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct Chat {
    pub id: String,
    #[serde(default)]
    pub title: String,
    pub user_id: Option<String>,
    pub created_at: Option<String>,
    #[serde(rename = "type")]
    pub chat_type: Option<String>,
    pub shared_options: Option<SharedOptions>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextContentBlock {
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileContentBlock {
    pub file_id: Option<String>,
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text(TextContentBlock),
    File(FileContentBlock),
    #[serde(other)]
    Unknown,
}

impl ContentBlock {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text(t) => Some(&t.text),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub id: Option<String>,
    pub role: String,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Deserialize)]
pub struct Interaction {
    pub id: String,
    pub chat_id: String,
    pub status: String,
    pub created_at: Option<String>,
    pub interaction_mode: Option<String>,
    pub model_choice: Option<String>,
    #[serde(default)]
    pub data_sources: Vec<Value>,
    pub feedback: Option<String>,
    pub feedback_description: Option<String>,
    #[serde(default)]
    pub responses: Option<Vec<Message>>,
}

impl Interaction {
    pub fn is_completed(&self) -> bool {
        self.status.eq_ignore_ascii_case("completed")
    }

    pub fn is_error(&self) -> bool {
        self.status.eq_ignore_ascii_case("error")
    }

    pub fn is_stopped(&self) -> bool {
        self.status.eq_ignore_ascii_case("stopped")
    }

    pub fn is_terminal(&self) -> bool {
        self.is_completed() || self.is_error() || self.is_stopped()
    }

    pub fn assistant_text(&self) -> Option<String> {
        self.responses.as_ref().and_then(|msgs| {
            let text: Vec<String> = msgs
                .iter()
                .filter(|m| m.role == "assistant")
                .map(|m| m.text_content())
                .filter(|t| !t.is_empty())
                .collect();
            if text.is_empty() {
                None
            } else {
                Some(text.join("\n\n"))
            }
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct ChatWithMessages {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub interactions: Vec<Interaction>,
}

#[derive(Debug, Deserialize)]
pub struct Artifact {
    pub id: Option<String>,
    pub download_url: Option<String>,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size: Option<u64>,
    pub created_at: Option<String>,
    pub artifact_type: Option<String>,
}

// ── API client ─────────────────────────────────────────────────────────────────

/// Olly API client.
/// Uses standard Bearer token authentication like other Coralogix APIs.
pub struct OllyApi {
    client: CxClient,
}

impl OllyApi {
    /// Create a new OllyApi client.
    pub fn new(endpoint: &str, api_key: &str) -> Result<Self> {
        let client = CxClient::new(endpoint, api_key)?;
        Ok(Self { client })
    }

    // ── Chats ──────────────────────────────────────────────────────────────────

    pub async fn create_chat(&self) -> Result<Chat> {
        let body = json!({ "chat_type": "cli" });
        self.client.post(&format!("{CHATS_BASE}/"), &body).await
    }

    pub async fn get_chat(&self, chat_id: &str) -> Result<ChatWithMessages> {
        let path = format!("{CHATS_BASE}/{chat_id}");
        self.client
            .get(&path, &[("response_format", "CONTENT_BLOCKS")])
            .await
    }

    pub async fn send_message(
        &self,
        chat_id: &str,
        content: &str,
        model_choice: &str,
        timeout_seconds: u32,
    ) -> Result<Interaction> {
        let path = format!("{CHATS_BASE}/{chat_id}/interactions/");
        let body = json!({
            "content": [InputContentBlock::text(content)],
            "interaction_mode": "skill",
            "model_choice": model_choice,
            "should_block": true,
            "timeout_seconds": timeout_seconds
        });
        self.client.post(&path, &body).await
    }

    pub async fn get_interaction(
        &self,
        chat_id: &str,
        interaction_id: &str,
    ) -> Result<Interaction> {
        let path = format!("{CHATS_BASE}/{chat_id}/interactions/{interaction_id}");
        self.client.get(&path, &[]).await
    }

    // ── Artifacts ──────────────────────────────────────────────────────────────

    pub async fn list_artifacts(&self) -> Result<Vec<Artifact>> {
        let path = format!("{ARTIFACTS_BASE}/");
        self.client.get(&path, &[]).await
    }

    pub async fn get_artifact(&self, artifact_id: &str) -> Result<Artifact> {
        let path = format!("{ARTIFACTS_BASE}/{artifact_id}");
        self.client.get(&path, &[]).await
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_chat_response() {
        let json = json!({
            "id": "chat-uuid-123",
            "title": "",
            "user_id": "user-456",
            "created_at": "2024-01-01T00:00:00Z",
            "shared_options": {
                "shared_type": "private",
                "shared_user_ids": null
            },
            "type": "cli",
            "metadata": null
        });
        let chat: Chat = serde_json::from_value(json).unwrap();
        assert_eq!(chat.id, "chat-uuid-123");
        assert_eq!(chat.title, "");
        assert_eq!(chat.user_id.as_deref(), Some("user-456"));
        assert_eq!(chat.chat_type.as_deref(), Some("cli"));
    }

    #[test]
    fn deserialize_chat_minimal() {
        let json = json!({
            "id": "chat-123"
        });
        let chat: Chat = serde_json::from_value(json).unwrap();
        assert_eq!(chat.id, "chat-123");
        assert_eq!(chat.title, "");
    }

    #[test]
    fn deserialize_interaction_in_progress() {
        let json = json!({
            "id": "interaction-123",
            "chat_id": "chat-456",
            "created_at": "2024-01-01T00:00:00Z",
            "status": "IN_PROGRESS",
            "interaction_mode": "AGENTIC",
            "model_choice": "DEFAULT",
            "data_sources": [],
            "feedback": null,
            "feedback_description": null,
            "responses": null
        });
        let interaction: Interaction = serde_json::from_value(json).unwrap();
        assert_eq!(interaction.id, "interaction-123");
        assert_eq!(interaction.chat_id, "chat-456");
        assert_eq!(interaction.status, "IN_PROGRESS");
        assert!(!interaction.is_terminal());
        assert!(interaction.responses.is_none());
    }

    #[test]
    fn deserialize_interaction_completed() {
        let json = json!({
            "id": "interaction-123",
            "chat_id": "chat-456",
            "status": "COMPLETED",
            "responses": [
                {
                    "id": "msg-789",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Here are the alerts..."}]
                }
            ]
        });
        let interaction: Interaction = serde_json::from_value(json).unwrap();
        assert!(interaction.is_completed());
        assert!(interaction.is_terminal());
        assert_eq!(
            interaction.assistant_text().as_deref(),
            Some("Here are the alerts...")
        );
    }

    #[test]
    fn deserialize_interaction_error() {
        let json = json!({
            "id": "interaction-123",
            "chat_id": "chat-456",
            "status": "error",
            "responses": null
        });
        let interaction: Interaction = serde_json::from_value(json).unwrap();
        assert!(interaction.is_error());
        assert!(interaction.is_terminal());
    }

    #[test]
    fn deserialize_interaction_stopped() {
        let json = json!({
            "id": "interaction-123",
            "chat_id": "chat-456",
            "status": "stopped",
            "responses": null
        });
        let interaction: Interaction = serde_json::from_value(json).unwrap();
        assert!(interaction.is_stopped());
        assert!(interaction.is_terminal());
    }

    #[test]
    fn deserialize_message_with_text() {
        let json = json!({
            "id": "msg-123",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello!"},
                {"type": "text", "text": "How can I help?"}
            ]
        });
        let msg: Message = serde_json::from_value(json).unwrap();
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.text_content(), "Hello!\nHow can I help?");
    }

    #[test]
    fn deserialize_message_with_file() {
        let json = json!({
            "id": "msg-123",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Here's the chart:"},
                {"type": "file", "file_id": "file-456", "filename": "chart.png"}
            ]
        });
        let msg: Message = serde_json::from_value(json).unwrap();
        assert_eq!(msg.content.len(), 2);
        match &msg.content[1] {
            ContentBlock::File(f) => {
                assert_eq!(f.file_id.as_deref(), Some("file-456"));
                assert_eq!(f.filename.as_deref(), Some("chart.png"));
            }
            _ => panic!("Expected file content block"),
        }
    }

    #[test]
    fn deserialize_message_with_unknown_type() {
        let json = json!({
            "id": "msg-123",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "some_new_type", "data": "whatever"}
            ]
        });
        let msg: Message = serde_json::from_value(json).unwrap();
        assert_eq!(msg.content.len(), 2);
        assert!(matches!(msg.content[1], ContentBlock::Unknown));
    }

    #[test]
    fn deserialize_chat_with_messages() {
        let json = json!({
            "id": "chat-123",
            "title": "My Chat",
            "messages": [
                {
                    "id": "msg-1",
                    "role": "user",
                    "content": [{"type": "text", "text": "Hello"}]
                },
                {
                    "id": "msg-2",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Hi there!"}]
                }
            ],
            "interactions": []
        });
        let chat: ChatWithMessages = serde_json::from_value(json).unwrap();
        assert_eq!(chat.id, "chat-123");
        assert_eq!(chat.title, "My Chat");
        assert_eq!(chat.messages.len(), 2);
    }

    #[test]
    fn deserialize_artifact() {
        let json = json!({
            "id": "artifact-123",
            "download_url": "https://storage.example.com/artifact-123?token=xyz",
            "filename": "chart.png",
            "content_type": "image/png",
            "size": 12345
        });
        let artifact: Artifact = serde_json::from_value(json).unwrap();
        assert_eq!(artifact.id.as_deref(), Some("artifact-123"));
        assert!(artifact.download_url.is_some());
        assert_eq!(artifact.filename.as_deref(), Some("chart.png"));
    }

    #[test]
    fn deserialize_artifact_minimal() {
        let json = json!({});
        let artifact: Artifact = serde_json::from_value(json).unwrap();
        assert!(artifact.id.is_none());
        assert!(artifact.download_url.is_none());
    }

    #[test]
    fn input_content_block_serializes_correctly() {
        let block = InputContentBlock::text("Hello world");
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "input_text");
        assert_eq!(json["text"], "Hello world");
    }
}
