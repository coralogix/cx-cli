use anyhow::Result;
use inquire::{Password, PasswordDisplayMode, Select, Text};

use crate::config::{
    load_config, save_config, save_profile, CredentialStorage, OutputFormat, Profile, Region,
};
use crate::keyring_store;

const REGIONS: &[&str] = &["us1", "us2", "eu1", "eu2", "ap1", "ap2", "ap3", "stg1"];

const OUTPUT_FORMATS: &[&str] = &["text", "json", "agents"];

const CREDENTIAL_STORAGE_OPTIONS: &[&str] = &["file", "os-store (encrypted)"];

pub fn run(profile_name: Option<String>) -> Result<()> {
    let name = profile_name.unwrap_or_else(|| "default".to_string());

    println!("Configuring profile '{name}'\n");

    let api_key = Password::new("Coralogix API key:")
        .with_display_mode(PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()?;

    let region_str = Select::new("Region:", REGIONS.to_vec())
        .with_starting_cursor(2) // eu1
        .prompt()?;
    let region: Region = region_str.parse()?;

    let label = Text::new("Label (e.g. 'prod'):").prompt_skippable()?;
    let label = label.filter(|s| !s.is_empty());

    let team_id = Text::new("Coralogix team ID (required for search-fields):")
        .prompt_skippable()?;
    let team_id = team_id.filter(|s| !s.is_empty());

    let openai_api_key = Password::new("OpenAI API key (optional):")
        .with_display_mode(PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt_skippable()?;
    let openai_api_key = openai_api_key.filter(|s| !s.is_empty());

    let storage_choice = Select::new(
        "Where should API keys be stored?",
        CREDENTIAL_STORAGE_OPTIONS.to_vec(),
    )
    .with_help_message("'file' stores in profile config (0600 perms). 'os-store' uses the OS credential store (macOS Keychain, Windows Credential Manager, etc.)")
    .with_starting_cursor(0) // file
    .prompt()?;

    let credential_storage = if storage_choice.starts_with("os-store") {
        CredentialStorage::OsStore
    } else {
        CredentialStorage::File
    };

    match credential_storage {
        CredentialStorage::OsStore => {
            keyring_store::store_secret(&name, "api_key", &api_key)?;
            if let Some(ref oai_key) = openai_api_key {
                keyring_store::store_secret(&name, "openai_api_key", oai_key)?;
            }
            let profile = Profile {
                credential_storage,
                api_key: None,
                region,
                label,
                team_id,
                openai_api_key: None,
            };
            save_profile(&name, &profile)?;
        }
        CredentialStorage::File => {
            // Clean up any leftover keychain entries from a previous config.
            keyring_store::delete_profile(&name);
            let profile = Profile {
                credential_storage,
                api_key: Some(api_key),
                region,
                label,
                team_id,
                openai_api_key,
            };
            save_profile(&name, &profile)?;
        }
    }

    // Update global config with the chosen default output format.
    let mut global_config = load_config().unwrap_or_default();
    let current_idx = OUTPUT_FORMATS
        .iter()
        .position(|&f| f == global_config.default_output_format.as_str())
        .unwrap_or(0);
    let format_str = Select::new("Default output format:", OUTPUT_FORMATS.to_vec())
        .with_starting_cursor(current_idx)
        .prompt()?;
    global_config.default_output_format = match format_str {
        "json" => OutputFormat::Json,
        "agents" => OutputFormat::Agents,
        _ => OutputFormat::Text,
    };
    save_config(&global_config)?;

    let cx_dir = crate::config::config_dir()?;
    let storage_desc = match credential_storage {
        CredentialStorage::File => "profile file",
        CredentialStorage::OsStore => "OS credential store",
    };
    println!(
        "\nProfile '{name}' saved to {}\nAPI key stored in {storage_desc}",
        cx_dir.display()
    );
    Ok(())
}
