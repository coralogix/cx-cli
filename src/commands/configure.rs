use anyhow::Result;
use inquire::{Password, PasswordDisplayMode, Select, Text};

use crate::config::{load_config, save_config, save_profile, OutputFormat, Profile, Region};
use crate::keyring_store;

const REGIONS: &[&str] = &["us1", "us2", "eu1", "eu2", "ap1", "ap2", "ap3", "stg1"];

const OUTPUT_FORMATS: &[&str] = &["text", "json", "agents"];

pub async fn run(profile_name: Option<String>, no_keyring: bool) -> Result<()> {
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

    if no_keyring {
        let profile = Profile {
            api_key: Some(api_key),
            region,
            label,
            team_id,
            openai_api_key,
        };
        save_profile(&name, &profile)?;
    } else {
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
    }

    // Update global config with the chosen default output format.
    let mut global_config = load_config().unwrap_or_default();
    let current_idx = OUTPUT_FORMATS
        .iter()
        .position(|&f| f == format!("{:?}", global_config.default_output_format).to_lowercase())
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
    let storage = if no_keyring { "profile file" } else { "system keyring" };
    println!(
        "\nProfile '{name}' saved to {}\nAPI key stored in {storage}",
        cx_dir.display()
    );
    Ok(())
}
