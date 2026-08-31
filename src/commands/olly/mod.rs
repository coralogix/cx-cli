use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

pub mod api;
mod artifact;

use api::OllyApi;
use artifact::{download_content, ArtifactContent};

use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::render;
use crate::spill::{self, maybe_spill, SpillOutcome};

// ── Subcommand runners ─────────────────────────────────────────────────────────

/// Send a message to the AI assistant.
///
/// Creates a new chat if `chat_id` is None, otherwise continues an existing chat.
/// Uses blocking mode to wait for the response.
#[allow(clippy::too_many_arguments)]
pub async fn run_ask(
    targets: &[Arc<ExecutionTarget>],
    message: &str,
    chat_id: Option<&str>,
    model: &str,
    timeout: u32,
    agent_to_agent_mode: bool,
    output: OutputFormat,
) -> Result<()> {
    // Olly is single-profile only - chats belong to a specific user/team
    if targets.len() > 1 {
        bail!("cx olly does not support multi-profile. Use a single profile with -p <profile>.");
    }

    let target = &targets[0];
    let api = OllyApi::new(&target.cfg.endpoint, &target.cfg.api_key)?;

    // Create a new chat if no chat_id provided
    let chat_id = match chat_id {
        Some(id) => id.to_string(),
        None => {
            eprintln!("{}", "Creating new chat...".dimmed());
            let chat = api.create_chat().await?;
            chat.id
        }
    };

    eprintln!("{}", "Sending message...".dimmed());
    let interaction = api
        .send_message(&chat_id, message, model, timeout, agent_to_agent_mode)
        .await?;

    // Olly is single-profile only, so unlike other command groups this uses
    // the resolved target directly rather than looking one up by profile name.
    target
        .emit_console_link(|base| crate::console_url::olly_chat_url(base, &chat_id))
        .await;

    // Render based on output format
    match output {
        OutputFormat::Json => {
            let response = interaction_to_json(&interaction, &chat_id);
            render::render_json(&[response])?;
        }
        OutputFormat::Toon => {
            let response = interaction_to_json(&interaction, &chat_id);
            let toon = toon_encode(&[response])
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            render_ask_text(&chat_id, &interaction)?;
        }
    }

    Ok(())
}

/// Processed artifact content after download.
enum ProcessedContent {
    /// Valid JSON content - can use spill logic.
    Json(Vec<Value>),
    /// Plain text content (not valid JSON) - saved to file.
    Text { path: std::path::PathBuf },
    /// No content available.
    None,
}

/// Get an artifact and download its content.
pub async fn run_artifacts_get(
    targets: &[Arc<ExecutionTarget>],
    artifact_id: &str,
    output: OutputFormat,
    max_direct: Option<usize>,
    temp_dir: &str,
) -> Result<()> {
    // Olly is single-profile only
    if targets.len() > 1 {
        bail!("cx olly does not support multi-profile. Use a single profile with -p <profile>.");
    }

    let target = &targets[0];
    let api = OllyApi::new(&target.cfg.endpoint, &target.cfg.api_key)?;

    eprintln!("{}", "Fetching artifact...".dimmed());
    let artifact = api.get_artifact(artifact_id).await?;

    // Download content from the presigned URL
    let raw_content = match &artifact.download_url {
        Some(url) => {
            eprintln!("{}", "Downloading content...".dimmed());
            Some(download_content(url).await?)
        }
        None => None,
    };

    // Process content: try JSON, fall back to text file
    let processed = match raw_content {
        Some(ArtifactContent::Text(text)) => {
            // Try to parse as JSON
            match serde_json::from_str::<Value>(&text) {
                Ok(json_value) => {
                    // Wrap in array for maybe_spill compatibility
                    let json_array = if json_value.is_array() {
                        json_value.as_array().unwrap().clone()
                    } else {
                        vec![json_value]
                    };
                    ProcessedContent::Json(json_array)
                }
                Err(_) => {
                    // Not JSON - save text to file
                    let path = write_text_artifact(&text, artifact_id, temp_dir)?;
                    ProcessedContent::Text { path }
                }
            }
        }
        Some(ArtifactContent::Binary) => ProcessedContent::None,
        None => ProcessedContent::None,
    };

    match output {
        OutputFormat::Json => {
            render_artifact_json(&artifact, &processed)?;
        }
        OutputFormat::Toon => {
            render_artifact_agents(&artifact, &processed, max_direct, temp_dir)?;
        }
        OutputFormat::Text => {
            render_artifact_text_output(&artifact, &processed, max_direct, temp_dir)?;
        }
    }

    Ok(())
}

/// Write text content to a temp file.
fn write_text_artifact(
    content: &str,
    artifact_id: &str,
    temp_dir: &str,
) -> Result<std::path::PathBuf> {
    let hash = short_hash_bytes(content.as_bytes());
    let filename = format!("{}_artifact_{artifact_id}_{hash}.txt", spill::FILE_PREFIX);
    let path = Path::new(temp_dir).join(&filename);
    std::fs::write(&path, content)?;
    Ok(path)
}

/// Simple hash for temp file naming.
fn short_hash_bytes(bytes: &[u8]) -> String {
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;
    let mut hash: u64 = FNV_OFFSET;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}").chars().take(8).collect()
}

// ── Output rendering by format ────────────────────────────────────────────────

fn render_artifact_json(artifact: &api::Artifact, processed: &ProcessedContent) -> Result<()> {
    let response = match processed {
        ProcessedContent::Json(json_array) => {
            json!({
                "id": artifact.id,
                "filename": artifact.filename,
                "content_type": artifact.content_type,
                "size": artifact.size,
                "content": json_array,
            })
        }
        ProcessedContent::Text { path } => {
            json!({
                "id": artifact.id,
                "filename": artifact.filename,
                "content_type": artifact.content_type,
                "size": artifact.size,
                "file": path.display().to_string(),
            })
        }
        ProcessedContent::None => {
            json!({
                "id": artifact.id,
                "filename": artifact.filename,
                "content_type": artifact.content_type,
                "size": artifact.size,
                "content": null,
            })
        }
    };
    render::render_json(&[response])
}

fn render_artifact_agents(
    artifact: &api::Artifact,
    processed: &ProcessedContent,
    max_direct: Option<usize>,
    temp_dir: &str,
) -> Result<()> {
    match processed {
        ProcessedContent::Json(json_array) => {
            // Use the original spill logic
            match maybe_spill(json_array, max_direct, temp_dir)? {
                SpillOutcome::Direct(json_str) => println!("{json_str}"),
                SpillOutcome::Spilled { path, count } => {
                    println!(
                        "{count} results retrieved. Results written to: {}",
                        path.display()
                    );
                }
            }
        }
        ProcessedContent::Text { path } => {
            let response = json!({
                "id": artifact.id,
                "filename": artifact.filename,
                "file": path.display().to_string(),
            });
            println!("{}", serde_json::to_string_pretty(&[response])?);
        }
        ProcessedContent::None => {
            let response = json!({
                "id": artifact.id,
                "filename": artifact.filename,
                "content": null,
            });
            println!("{}", serde_json::to_string_pretty(&[response])?);
        }
    }
    Ok(())
}

fn render_artifact_text_output(
    artifact: &api::Artifact,
    processed: &ProcessedContent,
    max_direct: Option<usize>,
    temp_dir: &str,
) -> Result<()> {
    // Print metadata header
    if let Some(id) = &artifact.id {
        println!("{} {}", "Artifact ID:".dimmed(), id.cyan());
    }
    if let Some(filename) = &artifact.filename {
        println!("{} {}", "Filename:".dimmed(), filename);
    }
    if let Some(content_type) = &artifact.content_type {
        println!("{} {}", "Type:".dimmed(), content_type);
    }
    if let Some(size) = artifact.size {
        println!("{} {} bytes", "Size:".dimmed(), size);
    }
    println!();

    match processed {
        ProcessedContent::Json(json_array) => {
            // Use spill logic for JSON
            match maybe_spill(json_array, max_direct, temp_dir)? {
                SpillOutcome::Direct(json_str) => {
                    println!("{}", "Content:".green());
                    println!("{json_str}");
                }
                SpillOutcome::Spilled { path, count } => {
                    println!(
                        "{} {} ({} items)",
                        "Content written to:".green(),
                        path.display(),
                        count
                    );
                }
            }
        }
        ProcessedContent::Text { path } => {
            println!("{} {}", "Text content saved to:".green(), path.display());
        }
        ProcessedContent::None => {
            println!("{}", "No content available.".yellow());
        }
    }

    Ok(())
}

/// List all artifacts.
pub async fn run_artifacts_list(
    targets: &[Arc<ExecutionTarget>],
    output: OutputFormat,
) -> Result<()> {
    // Olly is single-profile only
    if targets.len() > 1 {
        bail!("cx olly does not support multi-profile. Use a single profile with -p <profile>.");
    }

    let target = &targets[0];
    let api = OllyApi::new(&target.cfg.endpoint, &target.cfg.api_key)?;

    eprintln!("{}", "Fetching artifacts...".dimmed());
    let artifacts = api.list_artifacts().await?;

    match output {
        OutputFormat::Json => {
            let response: Vec<Value> = artifacts.iter().map(artifact_to_json).collect();
            render::render_json(&response)?;
        }
        OutputFormat::Toon => {
            let response: Vec<Value> = artifacts.iter().map(artifact_to_json).collect();
            let toon =
                toon_encode(&response).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            render_artifacts_list_text(&artifacts)?;
        }
    }

    Ok(())
}

// ── Rendering helpers ──────────────────────────────────────────────────────────

fn interaction_to_json(interaction: &api::Interaction, chat_id: &str) -> Value {
    let mut response = json!({
        "chat_id": chat_id,
        "interaction_id": interaction.id,
        "status": interaction.status,
    });

    if let Some(text) = interaction.assistant_text() {
        response
            .as_object_mut()
            .unwrap()
            .insert("response".to_string(), Value::String(text));
    }

    if let Some(mode) = &interaction.interaction_mode {
        response
            .as_object_mut()
            .unwrap()
            .insert("interaction_mode".to_string(), Value::String(mode.clone()));
    }

    if let Some(model) = &interaction.model_choice {
        response
            .as_object_mut()
            .unwrap()
            .insert("model_choice".to_string(), Value::String(model.clone()));
    }

    response
}

fn render_ask_text(chat_id: &str, interaction: &api::Interaction) -> Result<()> {
    // Print chat info for follow-up
    println!("{} {}", "Chat ID:".dimmed(), chat_id.cyan());

    // Print status if not completed
    if !interaction.is_completed() {
        println!("{} {}", "Status:".dimmed(), interaction.status.yellow());
    }

    println!();

    // Print assistant response
    match interaction.assistant_text() {
        Some(text) => {
            println!("{text}");
        }
        None => {
            if interaction.is_error() {
                println!("{}", "Generation encountered an error.".yellow());
            } else if interaction.is_stopped() {
                println!("{}", "Generation was stopped.".yellow());
            } else {
                println!("{}", "No response received.".yellow());
            }
        }
    }

    Ok(())
}

fn artifact_to_json(artifact: &api::Artifact) -> Value {
    json!({
        "id": artifact.id,
        "download_url": artifact.download_url,
        "filename": artifact.filename,
        "content_type": artifact.content_type,
        "size": artifact.size,
    })
}

fn render_artifacts_list_text(artifacts: &[api::Artifact]) -> Result<()> {
    use tabled::{builder::Builder, settings::Style};

    if artifacts.is_empty() {
        println!("{}", "No artifacts found.".yellow());
        return Ok(());
    }

    let mut builder = Builder::default();
    builder.push_record(["ID", "FILENAME", "TYPE", "SIZE", "CREATED"]);
    for artifact in artifacts {
        builder.push_record([
            artifact.id.clone().unwrap_or_else(|| "-".to_string()),
            artifact.filename.clone().unwrap_or_else(|| "-".to_string()),
            artifact
                .artifact_type
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            artifact
                .size
                .map(|s| format!("{} B", s))
                .unwrap_or_else(|| "-".to_string()),
            artifact
                .created_at
                .clone()
                .unwrap_or_else(|| "-".to_string()),
        ]);
    }

    let table = builder.build().with(Style::blank()).to_string();
    println!("{table}");

    Ok(())
}
