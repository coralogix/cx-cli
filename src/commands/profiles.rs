use anyhow::Result;
use inquire::{Confirm, Password, PasswordDisplayMode, Select, Text};

use crate::config::{
    load_config, load_profile, profile_file, profiles_dir, save_config, save_profile, AuthKind,
    CredentialStorage, OutputFormat, Profile, Region,
};
use crate::keyring_store;
use crate::oauth;

// ── Option lists ──────────────────────────────────────────────────────────────

const AUTH_METHODS: &[&str] = &["OAuth (browser login)", "API key (paste manually)"];

const REGIONS: &[&str] = &[
    "us1", "us2", "us3", "eu1", "eu2", "ap1", "ap2", "ap3", "stg1",
];

/// Region list for OAuth mode: same as REGIONS plus a custom option.
const OAUTH_REGIONS: &[&str] = &[
    "us1",
    "us2",
    "us3",
    "eu1",
    "eu2",
    "ap1",
    "ap2",
    "ap3",
    "stg1",
    "Custom (specify URL + client ID)",
];

const OUTPUT_FORMATS: &[&str] = &["text", "json", "agents"];

const CREDENTIAL_STORAGE_OPTIONS: &[&str] = &["file", "os-store (encrypted)"];

// ── List ──────────────────────────────────────────────────────────────────────

pub fn run_list() -> Result<()> {
    let dir = profiles_dir()?;
    let global_config = load_config().unwrap_or_default();

    if !dir.exists() {
        println!("No profiles configured. Run `cx profiles add` to create one.");
        return Ok(());
    }

    let mut entries: Vec<(String, Profile)> = std::fs::read_dir(&dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? != "toml" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            let profile = load_profile(&name).ok()?;
            Some((name, profile))
        })
        .collect();

    if entries.is_empty() {
        println!("No profiles configured. Run `cx profiles add` to create one.");
        return Ok(());
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Column widths
    let name_w = entries
        .iter()
        .map(|(n, _)| n.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let label_w = entries
        .iter()
        .map(|(_, p)| p.label.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(5)
        .max(5);
    let region_w = entries
        .iter()
        .map(|(_, p)| p.region.to_string().len())
        .max()
        .unwrap_or(6)
        .max(6);

    println!(
        "{:<name_w$}  {:<label_w$}  {:<region_w$}  {:<8}  DEFAULT",
        "NAME",
        "LABEL",
        "REGION",
        "AUTH",
        name_w = name_w,
        label_w = label_w,
        region_w = region_w,
    );
    println!(
        "{:<name_w$}  {:<label_w$}  {:<region_w$}  {:<8}  -------",
        "-".repeat(name_w),
        "-".repeat(label_w),
        "-".repeat(region_w),
        "--------",
        name_w = name_w,
        label_w = label_w,
        region_w = region_w,
    );

    for (name, profile) in &entries {
        let label = profile.label.as_deref().unwrap_or("-");
        let region = profile.region.to_string();
        let auth = match profile.auth {
            AuthKind::OAuth => "oauth",
            AuthKind::ApiKey => "api-key",
        };
        let is_default = if *name == global_config.default_profile {
            "yes"
        } else {
            ""
        };
        println!(
            "{:<name_w$}  {:<label_w$}  {:<region_w$}  {:<8}  {}",
            name,
            label,
            region,
            auth,
            is_default,
            name_w = name_w,
            label_w = label_w,
            region_w = region_w,
        );
    }

    Ok(())
}

// ── Add ───────────────────────────────────────────────────────────────────────

pub async fn run_add(profile_name: Option<String>) -> Result<()> {
    let name = profile_name.unwrap_or_else(|| "default".to_string());

    println!("Configuring profile '{name}'\n");

    let auth_choice = Select::new("Authentication method:", AUTH_METHODS.to_vec())
        .with_starting_cursor(0) // OAuth is the default
        .with_help_message(
            "OAuth opens your browser for secure login. \
             API key lets you paste credentials directly.",
        )
        .prompt()?;

    let use_oauth = auth_choice.starts_with("OAuth");

    let (profile, storage_desc) = if use_oauth {
        configure_oauth(&name).await?
    } else {
        configure_api_key(&name)?
    };

    save_profile(&name, &profile)?;

    // ── Common: default output format ──────────────────────────────────────────
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
    println!(
        "\nProfile '{name}' saved to {}\nCredentials stored in {storage_desc}",
        cx_dir.display()
    );
    Ok(())
}

// ── Delete ────────────────────────────────────────────────────────────────────

pub fn run_delete(profile_name: String, force: bool) -> Result<()> {
    let path = profile_file(&profile_name)?;
    if !path.exists() {
        anyhow::bail!("Profile '{profile_name}' not found.");
    }

    // Warn if deleting the default profile.
    let global_config = load_config().unwrap_or_default();
    let is_default = profile_name == global_config.default_profile;

    if !force {
        let warning = if is_default {
            format!(
                "Delete profile '{profile_name}'? This is your default profile.\n\
                 All stored credentials will be removed."
            )
        } else {
            format!(
                "Delete profile '{profile_name}'?\n\
                 All stored credentials will be removed."
            )
        };

        let confirmed = Confirm::new(&warning).with_default(false).prompt()?;

        if !confirmed {
            println!("Cancelled.");
            return Ok(());
        }
    }

    keyring_store::delete_profile(&profile_name);
    std::fs::remove_file(&path)?;
    println!("Profile '{profile_name}' deleted.");

    if is_default {
        println!(
            "Note: '{profile_name}' was your default profile. \
             Run `cx profiles set-default <name>` to set a new default."
        );
    }

    Ok(())
}

// ── Set Default ───────────────────────────────────────────────────────────────

pub fn run_set_default(profile_name: String) -> Result<()> {
    let path = profile_file(&profile_name)?;
    if !path.exists() {
        anyhow::bail!("Profile '{profile_name}' not found.");
    }

    let mut global_config = load_config().unwrap_or_default();

    if global_config.default_profile == profile_name {
        println!("Profile '{profile_name}' is already the default.");
        return Ok(());
    }

    global_config.default_profile = profile_name.clone();
    save_config(&global_config)?;

    println!("Default profile set to '{profile_name}'.");
    Ok(())
}

// ── OAuth configure path ──────────────────────────────────────────────────────

async fn configure_oauth(name: &str) -> Result<(Profile, &'static str)> {
    // Region / environment selection
    let region_str = Select::new("Region:", OAUTH_REGIONS.to_vec())
        .with_starting_cursor(3) // eu2
        .prompt()?;

    let is_custom = region_str.starts_with("Custom");

    let (region, base_url, client_id, oauth_client_id_for_profile) = if is_custom {
        let raw_url = Text::new("Base URL (e.g. https://api.myenv.coralogix.com):").prompt()?;
        let base_url = raw_url.trim_end_matches('/').to_string();
        let client_id = Text::new("OAuth client ID:").prompt()?;
        let region = Region::Custom(base_url.clone());
        // Store the client ID in the profile for custom environments.
        let client_id_for_profile = Some(client_id.clone());
        (region, base_url, client_id, client_id_for_profile)
    } else {
        let region: Region = region_str.parse()?;
        let base_url = region.api_endpoint().to_string();
        let client_id = oauth::client_id_for_region(region_str)
            .map(|s| s.to_string())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No OAuth client ID found for region '{region_str}'.\n\
                     Please select 'Custom' to provide your own base URL and client ID."
                )
            })?;
        // Known regions: client ID is hard-coded, don't store in profile TOML.
        (region, base_url, client_id, None)
    };

    let label = Text::new("Label (e.g. 'prod'):").prompt_skippable()?;
    let label = label.filter(|s| !s.is_empty());

    // Clean up any existing secrets before writing new ones.
    keyring_store::delete_profile(name);

    // ── Browser login ──────────────────────────────────────────────────────────
    println!();
    let tokens = oauth::browser_login(&base_url, &client_id).await?;
    println!("Login successful!");

    // Store tokens in the OS keyring.
    oauth::store_tokens(name, &tokens)?;

    // For custom environments, explicitly store the base URL so that token
    // refresh can reach the correct OIDC discovery endpoint even if the
    // Region display string is ever changed.  For known regions the base URL
    // is derived from `region.api_endpoint()` at runtime and need not be stored.
    let oauth_base_url_for_profile = if is_custom { Some(base_url) } else { None };

    let profile = Profile {
        auth: AuthKind::OAuth,
        // OAuth profiles always use the OS keyring; tokens are never in the TOML.
        credential_storage: CredentialStorage::OsStore,
        api_key: None,
        region,
        label,
        oauth_client_id: oauth_client_id_for_profile,
        oauth_base_url: oauth_base_url_for_profile,
    };

    Ok((profile, "OS credential store (OAuth tokens)"))
}

// ── API key configure path ────────────────────────────────────────────────────

fn configure_api_key(name: &str) -> Result<(Profile, &'static str)> {
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

    let storage_choice = Select::new(
        "Where should API keys be stored?",
        CREDENTIAL_STORAGE_OPTIONS.to_vec(),
    )
    .with_help_message(
        "'file' stores in profile config (0600 perms). \
         'os-store' uses the OS credential store (macOS Keychain, Windows Credential Manager).",
    )
    .with_starting_cursor(0)
    .prompt()?;

    let credential_storage = if storage_choice.starts_with("os-store") {
        CredentialStorage::OsStore
    } else {
        CredentialStorage::File
    };

    let (profile, storage_desc) = match credential_storage {
        CredentialStorage::OsStore => {
            keyring_store::store_secret(name, "api_key", &api_key)?;
            let profile = Profile {
                auth: AuthKind::ApiKey,
                credential_storage,
                api_key: None,
                region,
                label,
                oauth_client_id: None,
                oauth_base_url: None,
            };
            (profile, "OS credential store")
        }
        CredentialStorage::File => {
            // Clean up any leftover keychain entries from a previous config.
            keyring_store::delete_profile(name);
            let profile = Profile {
                auth: AuthKind::ApiKey,
                credential_storage,
                api_key: Some(api_key),
                region,
                label,
                oauth_client_id: None,
                oauth_base_url: None,
            };
            (profile, "profile file")
        }
    };

    Ok((profile, storage_desc))
}
