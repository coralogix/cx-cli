use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api_client::CxClient;
use crate::error::Result;

// ── Base paths ─────────────────────────────────────────────────────────────────

const CHATS_BASE: &str = "/api/v2/olly/v2/chats";
const ARTIFACTS_BASE: &str = "/api/v2/olly/artifacts";
const SCHEDULED_TASKS_BASE: &str = "/api/v2/olly/scheduled-tasks";
const INTERACTION_SOURCE_HEADER: (&str, &str) = ("interaction-source", "cli");

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

/// When a scheduled task runs. Neither interval makes it a one-off.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskSchedule {
    pub start_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_months: Option<u32>,
    pub enabled: bool,
}

/// The body of the scheduled-task `shared` endpoint. The API
/// requires `entity_ids` for `specific_entities` and rejects them otherwise.
fn shared_options_body(shared_type: &str, entity_ids: &[String]) -> Value {
    let mut body = json!({ "shared_type": shared_type });
    if !entity_ids.is_empty() {
        body["shared_entity_ids"] = json!(entity_ids);
    }
    body
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

/// A scheduled task, as far as the CLI reads it. Commands print the API's
/// full JSON; this is for the text view and for writing a schedule back.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub prompt: String,
    pub model_choice: Option<String>,
    pub reasoning_effort: Option<String>,
    pub kind: Option<String>,
    pub start_at: String,
    pub interval_seconds: Option<u64>,
    pub interval_months: Option<u32>,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub enabled: bool,
    pub notification: Option<TaskNotification>,
    pub metadata: Option<TaskMetadata>,
    pub shared_options: Option<TaskSharedOptions>,
    pub owner: Option<TaskOwner>,
    pub is_owner: Option<bool>,
}

impl ScheduledTask {
    /// The schedule the task has now, to write back with one field changed.
    pub fn schedule(&self) -> TaskSchedule {
        TaskSchedule {
            start_at: self.start_at.clone(),
            interval_seconds: self.interval_seconds,
            interval_months: self.interval_months,
            enabled: self.enabled,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskNotification {
    #[serde(rename = "type")]
    pub notification_type: String,
    pub condition: Option<String>,
    #[serde(default)]
    pub connector_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskMetadata {
    pub dashboard_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskSharedOptions {
    pub shared_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskOwner {
    pub entity_id: Option<String>,
    pub email: Option<String>,
}

/// One run of a scheduled task.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduledTaskRun {
    pub id: String,
    pub status: String,
    pub chat_id: Option<String>,
    pub interaction_id: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_reason: Option<String>,
}

// ── API client ─────────────────────────────────────────────────────────────────

/// Olly API client.
/// Uses standard Bearer token authentication like other Coralogix APIs.
pub struct OllyApi {
    client: CxClient,
}

impl OllyApi {
    /// Create an Olly API client from the configured shared HTTP client.
    pub fn from_client(client: CxClient) -> Self {
        Self { client }
    }

    // ── Chats ──────────────────────────────────────────────────────────────────

    pub async fn create_chat(&self) -> Result<Chat> {
        self.client
            .post_empty(&format!("{CHATS_BASE}/"), &[("chat_type", "ai_connector")])
            .await
    }

    pub async fn get_chat(&self, chat_id: &str) -> Result<ChatWithMessages> {
        let path = format!("{CHATS_BASE}/{chat_id}");
        self.client
            .get(&path, &[("response_format", "CONTENT_BLOCKS")])
            .await
    }

    /// Send a message to an Olly chat.
    ///
    /// `agent_to_agent_mode` signals that the caller is an AI agent (not a
    /// human): when `true`, Olly acts as a sub-agent - shorter responses, no
    /// charts/tables, asks clarifying questions instead of guessing, and
    /// relies on the parent agent's broader context. Defaults to `false` in
    /// the CLI since `cx olly ask` is used directly by humans as well as by
    /// agents; LLM/agent callers should pass `--agent-to-agent-mode` to opt in.
    pub async fn send_message(
        &self,
        chat_id: &str,
        content: &str,
        model_choice: &str,
        timeout_seconds: u32,
        agent_to_agent_mode: bool,
    ) -> Result<Interaction> {
        let path = format!("{CHATS_BASE}/{chat_id}/interactions/");
        let body = json!({
            "content": [InputContentBlock::text(content)],
            "interaction_mode": "skill",
            "model_choice": model_choice,
            "should_block": true,
            "timeout_seconds": timeout_seconds,
            "agent_to_agent_mode": agent_to_agent_mode
        });
        self.client
            .post_with_headers(&path, None, &body, &[INTERACTION_SOURCE_HEADER])
            .await
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

    /// List the caller's tasks, or with `shared` the tasks other users shared
    /// with them. `dashboard_id` narrows to the tasks that watch one
    /// dashboard; that endpoint takes no `kind`.
    pub async fn list_scheduled_tasks(
        &self,
        kind: Option<&str>,
        shared: bool,
        dashboard_id: Option<&str>,
    ) -> Result<Vec<Value>> {
        let mut params = vec![("scope", if shared { "shared" } else { "owner" })];
        let path = match dashboard_id {
            Some(id) => format!("{SCHEDULED_TASKS_BASE}/for-dashboard/{id}"),
            None => {
                if let Some(kind) = kind {
                    params.push(("kind", kind));
                }
                format!("{SCHEDULED_TASKS_BASE}/")
            }
        };
        self.client.get(&path, &params).await
    }

    /// The task as the API returns it. Parse it into [`ScheduledTask`] to read
    /// its fields.
    pub async fn get_scheduled_task(&self, task_id: &str) -> Result<Value> {
        self.client
            .get(&format!("{SCHEDULED_TASKS_BASE}/{task_id}"), &[])
            .await
    }

    /// Create a task from the API's create body. 409 means the per-user task
    /// quota is reached; 403 can also mean AI consent is off.
    pub async fn create_scheduled_task(&self, body: &Value) -> Result<Value> {
        self.client
            .post(&format!("{SCHEDULED_TASKS_BASE}/"), body)
            .await
    }

    /// Change any of `title`, `prompt`, `model_choice`, `reasoning_effort` and
    /// `notification`. The API requires at least one.
    pub async fn update_scheduled_task(&self, task_id: &str, body: &Value) -> Result<Value> {
        self.client
            .patch(&format!("{SCHEDULED_TASKS_BASE}/{task_id}"), body)
            .await
    }

    /// Replace the whole schedule. There is no endpoint for `enabled` alone.
    pub async fn set_scheduled_task_schedule(
        &self,
        task_id: &str,
        schedule: &TaskSchedule,
    ) -> Result<Value> {
        let path = format!("{SCHEDULED_TASKS_BASE}/{task_id}/schedule");
        self.client
            .put(&path, &serde_json::to_value(schedule)?)
            .await
    }

    /// Change who can see a task, its run history and its run chats.
    /// `shared_type` is `private`, `specific_entities` or `entire_organization`;
    /// the API requires `entity_ids` for `specific_entities` and rejects them
    /// otherwise.
    pub async fn share_scheduled_task(
        &self,
        task_id: &str,
        shared_type: &str,
        entity_ids: &[String],
    ) -> Result<Value> {
        let path = format!("{SCHEDULED_TASKS_BASE}/{task_id}/shared");
        self.client
            .patch(&path, &shared_options_body(shared_type, entity_ids))
            .await
    }

    pub async fn delete_scheduled_task(&self, task_id: &str) -> Result<()> {
        let _: Value = self
            .client
            .delete(&format!("{SCHEDULED_TASKS_BASE}/{task_id}"))
            .await?;
        Ok(())
    }

    /// Start a run now. The API answers 204 with no body, so the run's chat
    /// is only visible through [`Self::list_scheduled_task_runs`].
    pub async fn run_scheduled_task(&self, task_id: &str) -> Result<()> {
        let path = format!("{SCHEDULED_TASKS_BASE}/{task_id}/manual-run");
        let _: Value = self.client.post_empty(&path, &[]).await?;
        Ok(())
    }

    /// A task's runs, newest first. The API does not page them.
    pub async fn list_scheduled_task_runs(&self, task_id: &str) -> Result<Vec<Value>> {
        let path = format!("{SCHEDULED_TASKS_BASE}/{task_id}/scheduled-task-runs");
        self.client.get(&path, &[]).await
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scheduled_task_json() -> Value {
        json!({
            "id": "task-1",
            "team_id": "42",
            "title": "Daily errors",
            "prompt": "Summarize new errors",
            "model_choice": "gpt-5.2",
            "reasoning_effort": "medium",
            "start_at": "2026-11-20T09:00:00Z",
            "interval_seconds": 86400,
            "interval_months": null,
            "next_run_at": "2026-11-26T09:00:00Z",
            "last_run_at": "2026-11-25T09:00:03Z",
            "enabled": true,
            "notification": {
                "type": "on_condition",
                "condition": "error rate above 5%",
                "connector_ids": ["conn-1"]
            },
            "kind": "watch_dashboard",
            "metadata": {
                "dashboard_url": "https://team.app.coralogix.com/#/dashboards/abc",
                "dashboard_id": "abc"
            },
            "shared_options": {"shared_type": "entire_organization", "shared_entity_ids": null},
            "owner": {"entity_id": "user-1", "email": "owner@example.com"},
            "is_owner": true,
            "created_at": "2026-11-19T08:00:00Z",
            "updated_at": "2026-11-19T08:00:00Z",
            "scheduled_task_type": "recurring"
        })
    }

    #[test]
    fn deserialize_scheduled_task() {
        let task: ScheduledTask = serde_json::from_value(scheduled_task_json()).unwrap();
        assert_eq!(task.id, "task-1");
        assert_eq!(task.interval_seconds, Some(86400));
        assert_eq!(task.interval_months, None);
        assert_eq!(task.kind.as_deref(), Some("watch_dashboard"));
        let notification = task.notification.as_ref().unwrap();
        assert_eq!(notification.notification_type, "on_condition");
        assert_eq!(notification.connector_ids, ["conn-1"]);
        assert_eq!(
            task.owner.as_ref().unwrap().email.as_deref(),
            Some("owner@example.com")
        );
        assert_eq!(
            task.shared_options.as_ref().unwrap().shared_type.as_deref(),
            Some("entire_organization")
        );
    }

    #[test]
    fn deserialize_one_off_scheduled_task_with_nulls() {
        let task: ScheduledTask = serde_json::from_value(json!({
            "id": "task-2",
            "title": "Once",
            "prompt": "Check the deploy",
            "start_at": "2026-12-01T09:00:00Z",
            "interval_seconds": null,
            "interval_months": null,
            "next_run_at": null,
            "last_run_at": null,
            "enabled": false,
            "notification": null,
            "metadata": null
        }))
        .unwrap();
        assert!(!task.enabled);
        assert!(task.notification.is_none());
        assert!(task.next_run_at.is_none());
    }

    #[test]
    fn schedule_keeps_the_task_cadence_and_omits_the_unset_interval() {
        let task: ScheduledTask = serde_json::from_value(scheduled_task_json()).unwrap();
        let mut schedule = task.schedule();
        schedule.enabled = false;
        assert_eq!(
            serde_json::to_value(&schedule).unwrap(),
            json!({
                "start_at": "2026-11-20T09:00:00Z",
                "interval_seconds": 86400,
                "enabled": false
            })
        );
    }

    #[test]
    fn deserialize_scheduled_task_run() {
        let failed: ScheduledTaskRun = serde_json::from_value(json!({
            "id": "run-1",
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
        }))
        .unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.error_reason.as_deref(), Some("quota_exceeded"));
        assert!(failed.chat_id.is_none());

        let succeeded: ScheduledTaskRun = serde_json::from_value(json!({
            "id": "run-2",
            "status": "succeeded",
            "chat_id": "chat-9",
            "interaction_id": "int-9",
            "started_at": "2026-11-25T09:00:00Z",
            "finished_at": "2026-11-25T09:02:10Z",
            "notification_verdict": {"kind": "issue", "reason": "Error rate is 7%."}
        }))
        .unwrap();
        assert_eq!(succeeded.chat_id.as_deref(), Some("chat-9"));
        assert_eq!(succeeded.interaction_id.as_deref(), Some("int-9"));
    }

    #[test]
    fn shared_options_body_sends_entity_ids_only_when_given() {
        assert_eq!(
            shared_options_body("private", &[]),
            json!({"shared_type": "private"})
        );
        assert_eq!(
            shared_options_body("specific_entities", &["a".to_string()]),
            json!({"shared_type": "specific_entities", "shared_entity_ids": ["a"]})
        );
    }

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
