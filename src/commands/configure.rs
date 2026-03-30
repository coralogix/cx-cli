use anyhow::Result;
use inquire::{Password, PasswordDisplayMode, Select, Text};

use crate::config::{
    load_config, save_config, save_profile, OutputFormat, Profile, Region, SecretStorage,
};
use crate::keyring_store;

const REGIONS: &[&str] = &["us1", "us2", "eu1", "eu2", "ap1", "ap2", "ap3", "stg1"];

const OUTPUT_FORMATS: &[&str] = &["text", "json", "agents"];

const SECRET_STORAGE_OPTIONS: &[&str] = &["keyring", "file"];

pub fn run(
    profile_name: Option<String>,
    secret_storage: Option<SecretStorage>,
) -> Result<()> {
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

    let use_keyring = match secret_storage {
        Some(SecretStorage::Keyring) => true,
        Some(SecretStorage::File) => false,
        None => {
            let choice = Select::new("Store API keys in:", SECRET_STORAGE_OPTIONS.to_vec())
                .with_starting_cursor(0) // keyring
                .prompt()?;
            choice == "keyring"
        }
    };

    if use_keyring {
        keyring_store::store_secret(&name, "api_key", &api_key)?;
        if let Some(ref oai_key) = openai_api_key {
            keyring_store::store_secret(&name, "openai_api_key", oai_key)?;
        }
        let profile = Profile {
            api_key: None,
            region,
            label,
            team_id,
            openai_api_key: None,
        };
        save_profile(&name, &profile)?;
    } else {
        // Clean up any leftover keyring entries from a previous keyring-based config.
        keyring_store::delete_profile(&name);
        let profile = Profile {
            api_key: Some(api_key),
            region,
            label,
            team_id,
            openai_api_key,
        };
        save_profile(&name, &profile)?;
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
    let storage = if use_keyring { "system keyring" } else { "profile file" };
    println!(
        "\nProfile '{name}' saved to {}\nAPI key stored in {storage}",
        cx_dir.display()
    );
    Ok(())
}
