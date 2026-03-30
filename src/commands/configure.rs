use std::io::{self, Write};

use anyhow::Result;

use crate::config::{load_config, save_config, save_profile, OutputFormat, Profile, Region};

pub async fn run(profile_name: Option<String>) -> Result<()> {
    let name = profile_name.unwrap_or_else(|| "default".to_string());

    println!("Configuring profile '{name}'");
    println!("Press Enter to keep existing value when shown in brackets.\n");

    let api_key = prompt("Coralogix API key")?;
    let region_str = prompt_with_default("Region (us1/us2/eu1/eu2/ap1/ap2)", "eu1")?;
    let label = prompt_optional("Label (optional, e.g. 'prod')")?;
    let team_id = prompt_optional("Coralogix team ID (cgx-team-id, required for search-fields)")?;
    let openai_api_key =
        prompt_optional("OpenAI API key (optional, can also be set via OPENAI_API_KEY env var)")?;

    let region: Region = region_str.parse()?;
    let profile = Profile {
        api_key,
        region,
        label,
        team_id,
        openai_api_key,
    };

    save_profile(&name, &profile)?;

    // Update global config with the chosen default output format.
    let mut global_config = load_config().unwrap_or_default();
    let current_format = format!("{:?}", global_config.default_output_format).to_lowercase();
    let format_str =
        prompt_with_default("Default output format (text/json/agents)", &current_format)?;
    global_config.default_output_format = match format_str.to_lowercase().as_str() {
        "json" => OutputFormat::Json,
        "agents" => OutputFormat::Agents,
        _ => OutputFormat::Text,
    };
    save_config(&global_config)?;

    let cx_dir = crate::config::config_dir()?;
    println!(
        "\nProfile '{name}' saved to {}",
        cx_dir.display()
    );
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

fn prompt_with_default(label: &str, default: &str) -> Result<String> {
    print!("{label} [{default}]: ");
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    let trimmed = s.trim();
    Ok(if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    })
}

fn prompt_optional(label: &str) -> Result<Option<String>> {
    let s = prompt(label)?;
    Ok(if s.is_empty() { None } else { Some(s) })
}
