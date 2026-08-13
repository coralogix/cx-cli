use anyhow::Result;
use inquire::{Confirm, Password, PasswordDisplayMode, Select, Text};

use crate::config::{
    has_managed_completions, list_profile_names, load_config, load_profile, profile_file,
    save_config, save_profile, AuthKind, CredentialStorage, OutputFormat, Profile, Region,
};
use crate::keyring_store;
use crate::oauth;
use crate::region::{region_from_url, RegionMatch};

// ── Option lists ──────────────────────────────────────────────────────────────

const AUTH_METHODS: &[&str] = &["OAuth (browser login)", "API key (paste manually)"];

/// Label for the "paste a URL and derive the region" option.
const PASTE_URL_OPTION: &str = "Paste a Coralogix URL";
/// Label for the manual custom-endpoint (BYOC / private-link) option.
const CUSTOM_ENDPOINT_OPTION: &str = "Custom endpoint (BYOC / private link)";

/// Unified, searchable region list shown for every auth method. The eight known
/// regions come first (typing filters them), followed by the URL-paste and
/// custom-endpoint escape hatches.
const REGION_OPTIONS: &[&str] = &[
    "us1",
    "us2",
    "us3",
    "eu1",
    "eu2",
    "ap1",
    "ap2",
    "ap3",
    PASTE_URL_OPTION,
    CUSTOM_ENDPOINT_OPTION,
];

/// Zero-based cursor position of `eu2` in [`REGION_OPTIONS`] (the default).
const REGION_DEFAULT_CURSOR: usize = 4;

/// Result of the interactive region prompt.
enum RegionChoice {
    /// A known region — either picked from the list or derived from a pasted URL.
    Known(Region),
    /// A manually entered custom endpoint (BYOC / private-link / unparseable URL).
    /// `base_url` has any trailing slash stripped.
    Custom { base_url: String },
}

const OUTPUT_FORMATS: &[&str] = &["text", "json", "agents"];

/// Storage backend choices presented to the user. The first element is the
/// label shown in the prompt; the second is the variant it maps to. Order
/// matters: the first entry is the default cursor position.
const CREDENTIAL_STORAGE_OPTIONS: &[(&str, CredentialStorage)] = &[
    ("file", CredentialStorage::File),
    ("os-store (encrypted)", CredentialStorage::OsStore),
];

fn select_credential_storage(prompt: &str, help_message: &str) -> Result<CredentialStorage> {
    let labels: Vec<&str> = CREDENTIAL_STORAGE_OPTIONS.iter().map(|(l, _)| *l).collect();
    let chosen = Select::new(prompt, labels)
        .with_help_message(help_message)
        .with_starting_cursor(0)
        .prompt()?;
    Ok(CREDENTIAL_STORAGE_OPTIONS
        .iter()
        .find(|(label, _)| *label == chosen)
        .map(|(_, storage)| *storage)
        .expect("inquire returns one of the labels we passed in"))
}

/// Prompt for a region using the unified searchable select.
///
/// The list filters as the user types. Choosing [`PASTE_URL_OPTION`] asks for a
/// Coralogix URL and derives the region from it; an unrecognised URL falls back
/// to manual endpoint entry (same as [`CUSTOM_ENDPOINT_OPTION`]). This is the
/// single region entry point shared by both the OAuth and API-key flows.
fn select_region_interactive() -> Result<RegionChoice> {
    let choice = Select::new("Region:", REGION_OPTIONS.to_vec())
        .with_starting_cursor(REGION_DEFAULT_CURSOR)
        .with_help_message(
            "Type to filter. Don't know your region? Choose \"Paste a Coralogix URL\".",
        )
        .prompt()?;

    if choice == PASTE_URL_OPTION {
        let raw = Text::new("Coralogix URL (e.g. https://myteam.app.eu2.coralogix.com):")
            .with_help_message("Paste the URL from your browser; we'll derive the region.")
            .prompt()?;
        match region_from_url(&raw) {
            RegionMatch::Known(region) => {
                println!("Detected region: {region}");
                Ok(RegionChoice::Known(region))
            }
            RegionMatch::Unresolved => {
                println!(
                    "Couldn't map that URL to a known region - \
                     enter your API endpoint manually."
                );
                prompt_custom_endpoint()
            }
        }
    } else if choice == CUSTOM_ENDPOINT_OPTION {
        prompt_custom_endpoint()
    } else {
        // A known region short-name from the list; infallible via FromStr.
        Ok(RegionChoice::Known(choice.parse()?))
    }
}

/// Prompt for a manual custom API endpoint (BYOC / private-link).
fn prompt_custom_endpoint() -> Result<RegionChoice> {
    let raw_url = Text::new("Base URL (e.g. https://api.myenv.coralogix.com):").prompt()?;
    let base_url = raw_url.trim_end_matches('/').to_string();
    Ok(RegionChoice::Custom { base_url })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Print a one-line refresh hint when the user has managed static completions.
/// Only shown when `cx completions install` has been used at least once.
fn hint_completions_refresh() {
    if has_managed_completions() {
        println!(
            "Tip: profile list changed - run `cx completions refresh` \
             to update static completion scripts."
        );
    }
}

// ── List ──────────────────────────────────────────────────────────────────────

pub fn run_list() -> Result<()> {
    let global_config = load_config().unwrap_or_default();

    let names = list_profile_names()?;

    let entries: Vec<(String, Profile)> = names
        .into_iter()
        .filter_map(|name| {
            let profile = load_profile(&name).ok()?;
            Some((name, profile))
        })
        .collect();

    if entries.is_empty() {
        println!("No profiles configured. Run `cx profiles add` to create one.");
        return Ok(());
    }

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
    let output_w = 6; // "OUTPUT" header length

    println!(
        "{:<name_w$}  {:<label_w$}  {:<region_w$}  {:<8}  {:<output_w$}  DEFAULT",
        "NAME",
        "LABEL",
        "REGION",
        "AUTH",
        "OUTPUT",
        name_w = name_w,
        label_w = label_w,
        region_w = region_w,
        output_w = output_w,
    );
    println!(
        "{:<name_w$}  {:<label_w$}  {:<region_w$}  {:<8}  {:<output_w$}  -------",
        "-".repeat(name_w),
        "-".repeat(label_w),
        "-".repeat(region_w),
        "--------",
        "-".repeat(output_w),
        name_w = name_w,
        label_w = label_w,
        region_w = region_w,
        output_w = output_w,
    );

    for (name, profile) in &entries {
        let label = profile.label.as_deref().unwrap_or("-");
        let region = profile.region.to_string();
        let auth = match profile.auth {
            AuthKind::OAuth => "oauth",
            AuthKind::ApiKey => "api-key",
        };
        let output_fmt = profile
            .default_output_format
            .map(|f| f.as_str())
            .unwrap_or("-");
        let is_default = if *name == global_config.default_profile {
            "yes"
        } else {
            ""
        };
        println!(
            "{:<name_w$}  {:<label_w$}  {:<region_w$}  {:<8}  {:<output_w$}  {}",
            name,
            label,
            region,
            auth,
            output_fmt,
            is_default,
            name_w = name_w,
            label_w = label_w,
            region_w = region_w,
            output_w = output_w,
        );
    }

    Ok(())
}

// ── Add ───────────────────────────────────────────────────────────────────────

pub async fn run_add(profile_name: Option<String>, set_default: bool) -> Result<()> {
    let is_first_profile = list_profile_names()?.is_empty();

    let name = match profile_name {
        Some(n) => {
            if n.is_empty() {
                anyhow::bail!("Profile name cannot be empty.");
            }
            n
        }
        None => {
            let mut prompt = Text::new("Profile name:")
                .with_help_message("A short identifier for this profile, e.g. 'prod' or 'staging'.")
                .with_validator(inquire::validator::MinLengthValidator::new(1));
            if is_first_profile {
                prompt = prompt.with_default("default");
            }
            prompt.prompt()?
        }
    };

    println!("Configuring profile '{name}'\n");

    let auth_choice = Select::new("Authentication method:", AUTH_METHODS.to_vec())
        .with_starting_cursor(0) // OAuth is the default
        .with_help_message(
            "OAuth opens your browser for secure login. \
             API key lets you paste credentials directly \
             (must be a Team Key or Personal Key - not a Send-Your-Data key).",
        )
        .prompt()?;

    let use_oauth = auth_choice.starts_with("OAuth");

    let (mut profile, storage_desc) = if use_oauth {
        configure_oauth(&name).await?
    } else {
        configure_api_key(&name)?
    };

    // ── Common: default output format (per-profile) ────────────────────────────
    let global_config = load_config().unwrap_or_default();
    let current_fmt = profile
        .default_output_format
        .unwrap_or(global_config.default_output_format);
    let current_idx = OUTPUT_FORMATS
        .iter()
        .position(|&f| f == current_fmt.as_str())
        .unwrap_or(0);
    let format_str = Select::new("Default output format:", OUTPUT_FORMATS.to_vec())
        .with_starting_cursor(current_idx)
        .prompt()?;
    profile.default_output_format = Some(match format_str {
        "json" => OutputFormat::Json,
        "agents" => OutputFormat::Agents,
        _ => OutputFormat::Text,
    });

    save_profile(&name, &profile)?;

    // On first profile creation, prompt for global safety settings and auto-set as default.
    if is_first_profile {
        println!("\n─── Global Safety Settings ───");
        println!("These apply to all profiles. Change later in ~/.cx/config.toml\n");

        let mut global_config = load_config().unwrap_or_default();

        global_config.allow_risky_commands =
            Confirm::new("Allow risky commands? (iam, archive write operations)")
                .with_default(true)
                .with_help_message(
                    "When disabled, write operations under 'iam' and 'archive' are blocked. \
             Read operations remain available.",
                )
                .prompt()?;

        global_config.olly_enabled = Confirm::new("Enable Olly AI assistant? (olly ask)")
            .with_default(true)
            .with_help_message("When disabled, 'olly ask' is blocked.")
            .prompt()?;

        global_config.default_profile = name.clone();
        save_config(&global_config)?;
    }

    if !is_first_profile {
        let mut global_config = load_config().unwrap_or_default();
        let should_set_default = if set_default {
            true
        } else {
            Confirm::new(&format!("Set '{name}' as the default profile?"))
                .with_default(false)
                .prompt()?
        };
        if should_set_default {
            global_config.default_profile = name.clone();
            save_config(&global_config)?;
        }
    }

    let cx_dir = crate::config::config_dir()?;
    println!(
        "\nProfile '{name}' saved to {}\nCredentials stored in {storage_desc}",
        cx_dir.display()
    );
    hint_completions_refresh();
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

    hint_completions_refresh();
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
    // Region / environment selection (searchable list, URL derivation, or BYOC).
    let is_custom;
    let (region, base_url, client_id, oauth_client_id_for_profile) =
        match select_region_interactive()? {
            RegionChoice::Custom { base_url } => {
                is_custom = true;
                let client_id = Text::new("OAuth client ID:").prompt()?;
                let region = Region::Custom(base_url.clone());
                // Store the client ID in the profile for custom environments.
                let client_id_for_profile = Some(client_id.clone());
                (region, base_url, client_id, client_id_for_profile)
            }
            RegionChoice::Known(region) => {
                is_custom = false;
                let base_url = region.api_endpoint().to_string();
                let region_name = region.to_string();
                let client_id = oauth::client_id_for_region(&region_name)
                    .map(|s| s.to_string())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No OAuth client ID found for region '{region_name}'.\n\
                             Please choose \"{CUSTOM_ENDPOINT_OPTION}\" to provide your own \
                             base URL and client ID."
                        )
                    })?;
                // Known regions: client ID is hard-coded, don't store in profile TOML.
                (region, base_url, client_id, None)
            }
        };

    let label = Text::new("Label (e.g. 'prod'):").prompt_skippable()?;
    let label = label.filter(|s| !s.is_empty());

    // Clean up any existing keyring entries before writing new ones, regardless
    // of which storage backend the user picks below.
    keyring_store::delete_profile(name);

    // ── Browser login ──────────────────────────────────────────────────────────
    println!();
    let tokens = oauth::browser_login(&base_url, &client_id).await?;
    println!("Login successful!");

    let credential_storage = select_credential_storage(
        "Where should OAuth tokens be stored?",
        "'file' stores tokens in the profile config (0600 perms). \
         'os-store' uses the OS credential store (macOS Keychain, Windows Credential Manager).",
    )?;

    // For custom environments, explicitly store the base URL so that token
    // refresh can reach the correct OIDC discovery endpoint even if the
    // Region display string is ever changed.  For known regions the base URL
    // is derived from `region.api_endpoint()` at runtime and need not be stored.
    let oauth_base_url_for_profile = if is_custom { Some(base_url) } else { None };

    let (oauth_tokens, storage_desc): (Option<_>, &'static str) = match credential_storage {
        CredentialStorage::OsStore => {
            oauth::store_tokens_keyring(name, &tokens)?;
            (None, "OS credential store (OAuth tokens)")
        }
        CredentialStorage::File => (
            Some(oauth::tokens_to_stored(&tokens)),
            "profile file (OAuth tokens)",
        ),
    };

    let profile = Profile {
        auth: AuthKind::OAuth,
        credential_storage,
        api_key: None,
        region,
        label,
        oauth_client_id: oauth_client_id_for_profile,
        oauth_base_url: oauth_base_url_for_profile,
        oauth_tokens,
        default_output_format: None,
        default_tier: None,
    };

    Ok((profile, storage_desc))
}

// ── API key configure path ────────────────────────────────────────────────────

fn configure_api_key(name: &str) -> Result<(Profile, &'static str)> {
    println!(
        "The API key must be a Team Key or a Personal Key.\n  \
         • Team Key:     Data Flow → API Keys → Team Keys\n  \
         • Personal Key: User menu (top-right) → Personal Keys\n\
         Send-Your-Data / ingress keys will NOT work for querying."
    );
    let api_key = Password::new("Coralogix API key (Team Key or Personal Key):")
        .with_display_mode(PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()?;

    let region = match select_region_interactive()? {
        RegionChoice::Known(region) => region,
        RegionChoice::Custom { base_url } => Region::Custom(base_url),
    };

    let label = Text::new("Label (e.g. 'prod'):").prompt_skippable()?;
    let label = label.filter(|s| !s.is_empty());

    let credential_storage = select_credential_storage(
        "Where should API keys be stored?",
        "'file' stores in profile config (0600 perms). \
         'os-store' uses the OS credential store (macOS Keychain, Windows Credential Manager).",
    )?;

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
                oauth_tokens: None,
                default_output_format: None,
                default_tier: None,
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
                oauth_tokens: None,
                default_output_format: None,
                default_tier: None,
            };
            (profile, "profile file")
        }
    };

    Ok((profile, storage_desc))
}
