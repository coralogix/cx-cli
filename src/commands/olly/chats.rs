use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::Value;

use super::api::{ChatWithMessages, OllyApi};
use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::render;
use crate::spill::{maybe_spill, SpillOutcome};

/// Chats and models belong to one user or team, so `cx olly` takes one profile.
fn single_api(targets: &[Arc<ExecutionTarget>]) -> Result<OllyApi> {
    if targets.len() > 1 {
        bail!("cx olly does not support multi-profile. Use a single profile with -p <profile>.");
    }
    Ok(OllyApi::from_client(targets[0].client.clone()))
}

fn str_field<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("-")
}

/// List chats, most recently active first.
pub async fn run_chats_list(
    targets: &[Arc<ExecutionTarget>],
    chat_type: Option<&str>,
    metadata: Option<&str>,
    limit: u32,
    offset: u32,
    output: OutputFormat,
) -> Result<()> {
    let api = single_api(targets)?;
    eprintln!("{}", "Fetching chats...".dimmed());
    let chats = api.list_chats(limit, offset, chat_type, metadata).await?;

    match output {
        OutputFormat::Json => render::render_json(&chats)?,
        OutputFormat::Toon => render::render_toon(&chats)?,
        OutputFormat::Text => {
            if chats.is_empty() {
                render::print_no_results("No chats found.");
                return Ok(());
            }
            let rows = chats
                .iter()
                .map(|chat| {
                    // `render_table` expects the profile name first and drops
                    // it for a single profile.
                    ["", "id", "title", "type", "created_at"]
                        .map(|key| str_field(chat, key).to_string())
                        .to_vec()
                })
                .collect();
            render::render_table(&["ID", "TITLE", "TYPE", "CREATED"], rows, false);
        }
    }
    Ok(())
}

/// Show a chat with its messages.
pub async fn run_chats_get(
    targets: &[Arc<ExecutionTarget>],
    chat_id: &str,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
) -> Result<()> {
    let api = single_api(targets)?;
    eprintln!("{}", "Fetching chat...".dimmed());
    let chat = api.get_chat(chat_id).await?;

    if matches!(output, OutputFormat::Json) {
        return render::render_json_auto(&[chat]);
    }
    let rows = [chat];
    if let SpillOutcome::Spilled { path, .. } = maybe_spill(&rows, max_direct, temp_dir)? {
        println!("Chat is too large to print. Written to: {}", path.display());
        return Ok(());
    }
    let [chat] = rows;
    if matches!(output, OutputFormat::Toon) {
        return render::render_toon(&[chat]);
    }
    print!("{}", format_conversation(&serde_json::from_value(chat)?));
    Ok(())
}

/// The conversation as text: who said what, in order.
fn format_conversation(chat: &ChatWithMessages) -> String {
    let mut out = format!("{} {}\n", "Chat ID:".dimmed(), chat.id.cyan());
    if !chat.title.is_empty() {
        out.push_str(&format!("{} {}\n", "Title:".dimmed(), chat.title));
    }
    if chat.messages.is_empty() {
        out.push_str(&format!("\n{}\n", "No messages.".yellow()));
    }
    for message in &chat.messages {
        let text = message.text_content();
        let text = if text.is_empty() {
            "(no text content)"
        } else {
            &text
        };
        out.push_str(&format!("\n{}\n{text}\n", message.role.bold()));
    }
    out
}

/// List the models `cx olly ask --model` accepts in this region.
pub async fn run_models(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    let api = single_api(targets)?;
    let models = api.list_models().await?;
    let rows: Vec<Value> = models.iter().cloned().map(Value::String).collect();

    match output {
        OutputFormat::Json => render::render_json(&rows)?,
        OutputFormat::Toon => render::render_toon(&rows)?,
        OutputFormat::Text => {
            for model in &models {
                println!("{model}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn conversation_lists_messages_in_order_with_roles() {
        colored::control::set_override(false);
        let chat: ChatWithMessages = serde_json::from_value(json!({
            "id": "chat-1",
            "title": "Checkout latency",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Why is it slow?"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "A deploy at 10:02."}]},
                {"role": "user", "content": [{"type": "file", "file_id": "f1"}]}
            ]
        }))
        .unwrap();
        assert_eq!(
            format_conversation(&chat),
            "Chat ID: chat-1\nTitle: Checkout latency\n\
             \nuser\nWhy is it slow?\n\
             \nassistant\nA deploy at 10:02.\n\
             \nuser\n(no text content)\n"
        );
    }
}
