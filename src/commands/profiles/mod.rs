use std::io::IsTerminal;

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

/// Label for the manual custom-endpoint (BYOC / private-link) option.
const CUSTOM_ENDPOINT_OPTION: &str = "Custom endpoint (BYOC / private link)";

/// Known regions shown in the picker, in display order. Each is rendered as
/// `({short-name}) {app_url_template}` so users can match against the URL they
/// see in the browser. The URL-paste and custom-endpoint escape hatches are
/// appended after these by [`region_picker_options`].
const PICKER_REGIONS: &[Region] = &[
    Region::Us1,
    Region::Us2,
    Region::Us3,
    Region::Eu1,
    Region::Eu2,
    Region::Ap1,
    Region::Ap2,
    Region::Ap3,
];

/// Zero-based cursor position of `eu2` in [`PICKER_REGIONS`] (the default).
const REGION_DEFAULT_CURSOR: usize = 4;

/// Picker label: `(eu2) app.eu2.coralogix.com`.
fn region_option_label(region: &Region) -> String {
    match region.app_url_template() {
        Some(template) => format!("({region}) {template}"),
        None => region.to_string(),
    }
}

fn region_picker_options() -> Vec<String> {
    PICKER_REGIONS
        .iter()
        .map(region_option_label)
        .chain([CUSTOM_ENDPOINT_OPTION.to_string()])
        .collect()
}

fn is_picker_region(region: &Region) -> bool {
    PICKER_REGIONS.iter().any(|r| r == region)
}

/// Result of the interactive region prompt.
// Debug is needed by the flag-resolution tests (`Result::expect_err`).
#[derive(Debug)]
enum RegionChoice {
    /// A known region — either picked from the list or derived from a pasted URL.
    Known(Region),
    /// A manually entered custom endpoint (BYOC / private-link / unparseable URL).
    /// `base_url` has any trailing slash stripped.
    Custom { base_url: String },
}

const OUTPUT_FORMATS: &[&str] = &["text", "json", "toon"];

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
/// The list filters as the user types, and pasting a Coralogix URL directly
/// into the filter auto-detects the region (or surfaces the custom-endpoint
/// option for an unrecognised one). Choosing [`CUSTOM_ENDPOINT_OPTION`] enters
/// the manual BYOC / private-link flow. This is the single region entry point
/// shared by both the OAuth and API-key flows.
fn select_region_interactive() -> Result<RegionChoice> {
    // `Select` doesn't hand back the filter text on selection, but we need it:
    // if the user pasted a URL and then chose the custom-endpoint option, we
    // should act on that URL instead of asking them to type it a second time.
    // The scorer runs on every keystroke, so record the last input it sees.
    let last_input = std::cell::RefCell::new(String::new());
    let scorer = |input: &str, option: &String, string_value: &str, idx: usize| -> Option<i64> {
        *last_input.borrow_mut() = input.to_string();
        match region_from_url(input) {
            // A URL that derives a region we actually list: surface only
            // that region. A derived region *not* in the list (e.g. a
            // staging URL) falls through to the custom-endpoint option below —
            // never to an empty list.
            RegionMatch::Known(region) if is_picker_region(&region) => {
                (region_option_label(&region) == string_value).then_some(i64::MAX)
            }
            // Unrecognized URL-ish input (BYOC / private-link / staging).
            // Surface only the custom-endpoint option. Schemeless hosts count
            // too — region_from_url accepts them — so gate on "looks like a
            // host", not on a scheme being present.
            _ => {
                if (input.contains("://") || input.contains('.'))
                    && string_value == CUSTOM_ENDPOINT_OPTION
                {
                    return Some(0);
                }
                (Select::<String>::DEFAULT_SCORER)(input, option, string_value, idx)
            }
        }
    };
    let choice = Select::new("Region / Coralogix URL:", region_picker_options())
        .with_starting_cursor(REGION_DEFAULT_CURSOR)
        .with_help_message("Enter your Coralogix URL to auto-detect the region, or type to filter.")
        .with_scorer(&scorer)
        .prompt()?;

    if choice == CUSTOM_ENDPOINT_OPTION {
        // If the filter already holds a URL, act on it directly rather than
        // prompting for the Base URL again: a recognised URL still resolves to
        // its region, anything else with a host becomes a custom endpoint.
        let typed = last_input.into_inner().trim().to_string();
        if typed.contains("://") || typed.contains('.') {
            return resolve_url_choice(&typed);
        }
        prompt_custom_endpoint()
    } else {
        let region = PICKER_REGIONS
            .iter()
            .find(|r| region_option_label(r) == choice)
            .cloned()
            .expect("inquire returns one of the labels we passed in");
        Ok(RegionChoice::Known(region))
    }
}

/// Resolve a user-supplied URL into a [`RegionChoice`] without further prompting:
/// a recognised Coralogix URL yields its region; an unrecognised URL that still
/// carries a host becomes a custom endpoint. Only a URL with no parseable host
/// falls back to the manual [`prompt_custom_endpoint`].
fn resolve_url_choice(raw: &str) -> Result<RegionChoice> {
    match region_from_url(raw) {
        // Only accept a derived region the picker actually lists — which is
        // exactly the set with hard-coded OAuth client IDs. A recognised but
        // *unlisted* region (e.g. staging `stg1`) has no client ID and no
        // picker row, so returning it here would later abort the OAuth flow at
        // the client-ID lookup. Route it through the custom-endpoint flow
        // instead, matching how the picker's own scorer treats such URLs.
        RegionMatch::Known(region) if is_picker_region(&region) => {
            println!("Detected region: {region}");
            Ok(RegionChoice::Known(region))
        }
        _ => match custom_base_url_from(raw) {
            Some(base_url) => {
                println!(
                    "Couldn't map that URL to a known region - \
                     using it as a custom endpoint: {base_url}"
                );
                Ok(RegionChoice::Custom { base_url })
            }
            None => {
                println!(
                    "Couldn't map that URL to a known region - \
                     enter your API endpoint manually."
                );
                prompt_custom_endpoint()
            }
        },
    }
}

/// Normalise a raw URL into a custom base URL: trailing slash stripped and an
/// `https://` scheme added when the input is scheme-less. Returns `None` when
/// the input has no parseable host.
///
/// The scheme is mandatory: a stored endpoint like `api.myenv.example.com`
/// (which `extract_host` happily parses by prepending a scheme internally)
/// would otherwise be written verbatim and then produce invalid request URLs
/// in `CxClient` and fail OAuth OIDC discovery.
fn custom_base_url_from(raw: &str) -> Option<String> {
    let cleaned = raw.trim().trim_end_matches('/');
    if cleaned.is_empty() {
        return None;
    }
    let with_scheme = if cleaned.contains("://") {
        cleaned.to_string()
    } else {
        format!("https://{cleaned}")
    };
    crate::region::extract_host(&with_scheme).map(|_| with_scheme)
}

/// Prompt for a manual custom API endpoint (BYOC / private-link).
///
/// Rejects input without a parseable host — an empty or garbage endpoint
/// would otherwise be written to the profile and fail every subsequent
/// command with an opaque HTTP error instead of a config error here.
fn prompt_custom_endpoint() -> Result<RegionChoice> {
    use inquire::validator::Validation;
    let raw_url = Text::new("Base URL (e.g. https://api.myenv.coralogix.com):")
        .with_validator(|input: &str| {
            if crate::region::extract_host(input).is_some() {
                Ok(Validation::Valid)
            } else {
                Ok(Validation::Invalid(
                    "Enter a URL with a host, e.g. https://api.myenv.coralogix.com".into(),
                ))
            }
        })
        .prompt()?;
    let base_url = custom_base_url_from(&raw_url).expect("validator guarantees a parseable host");
    Ok(RegionChoice::Custom { base_url })
}

// ── Flag resolution (per-value: flag/env → prompt) ───────────────────────────

/// Parse a `--region` flag value, rejecting anything that isn't a known
/// region short-name. `Region::from_str` is infallible (unknown strings become
/// `Region::Custom`), so without this guard a typo like `--region eu22` would
/// silently write a garbage endpoint to the profile.
fn parse_region_flag(raw: &str) -> Result<Region> {
    let region: Region = raw.parse().expect("Region::from_str is infallible");
    if matches!(region, Region::Custom(_)) {
        anyhow::bail!(
            "unknown region '{raw}' — expected one of us1, us2, us3, eu1, eu2, ap1, ap2, ap3, \
             or pass --url with your Coralogix URL"
        );
    }
    Ok(region)
}

/// Resolve the `--url` / `--region` flags into a [`RegionChoice`], if either
/// was supplied. Validation happens eagerly so a bad flag value fails before
/// any prompting starts. A `--url` that doesn't map to a picker-listed region
/// (BYOC / private link / staging) is used **verbatim** as a custom API
/// endpoint (trimmed, trailing slash stripped) — the customer supplied it
/// explicitly, so we never rewrite it, e.g. by inventing an `https://` scheme.
fn resolve_region_flags(url: Option<&str>, region: Option<&str>) -> Result<Option<RegionChoice>> {
    if let Some(raw) = region {
        return Ok(Some(RegionChoice::Known(parse_region_flag(raw)?)));
    }
    let Some(raw) = url else {
        return Ok(None);
    };
    match region_from_url(raw) {
        // Only accept regions the picker lists — the set with hard-coded OAuth
        // client IDs. A recognised but unlisted region (e.g. staging `stg1`)
        // routes through the custom-endpoint path, matching the interactive flow.
        RegionMatch::Known(region) if is_picker_region(&region) => {
            println!("Detected region: {region}");
            Ok(Some(RegionChoice::Known(region)))
        }
        _ => {
            if crate::region::extract_host(raw).is_none() {
                anyhow::bail!(
                    "'--url {raw}' is not a valid URL — pass your Coralogix URL \
                     (e.g. https://myteam.app.eu2.coralogix.com) or use --region"
                );
            }
            println!("Couldn't map '{raw}' to a known region — using it as a custom API endpoint.");
            let base_url = raw.trim().trim_end_matches('/').to_string();
            Ok(Some(RegionChoice::Custom { base_url }))
        }
    }
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

/// Flag surface for `cx profiles add`. Semantics in one line: flags answer
/// questions; unanswered questions get prompted; no terminal means unanswered
/// questions are errors.
pub struct AddArgs {
    /// Profile name (positional or `--profile`). Non-interactive default: "default".
    pub name: Option<String>,
    /// `--url`: Coralogix URL the region is derived from.
    pub url: Option<String>,
    /// `--region`: region short-name, alternative to `--url`.
    pub region: Option<String>,
    /// `--api-key` / `CX_API_KEY`.
    pub api_key: Option<String>,
    /// `--oauth`: use OAuth browser login, skipping the auth-method prompt.
    pub oauth: bool,
    /// `--force`: overwrite an existing profile without prompting.
    pub force: bool,
    /// `--set-default`: set this profile as the default without prompting.
    pub set_default: bool,
}

pub async fn run_add(args: AddArgs) -> Result<()> {
    let AddArgs {
        name,
        url,
        region,
        api_key,
        oauth,
        force,
        set_default,
    } = args;

    // Treat an empty/whitespace-only key (e.g. `--api-key ""` or an unset CI
    // secret expanding to nothing) as missing, so it's prompted for or errors
    // instead of silently saving an unusable profile.
    //
    // `--oauth` takes precedence over the key: CX_API_KEY is commonly exported
    // in shells, and a clap conflict would fire on the env value too, making
    // `--oauth` unusable for exactly the users most likely to want it.
    let api_key = api_key.filter(|k| !k.trim().is_empty()).filter(|_| !oauth);

    // Validate flag values before any prompting starts.
    let region_choice = resolve_region_flags(url.as_deref(), region.as_deref())?;

    // A fully-specified invocation (API key + region) never prompts, even on a
    // terminal — there are no unanswered questions left.
    let fully_specified = api_key.is_some() && region_choice.is_some();
    let interactive = std::io::stdin().is_terminal() && !fully_specified;

    let is_first_profile = list_profile_names()?.is_empty();

    let name = match name {
        Some(n) => {
            if n.is_empty() {
                anyhow::bail!("Profile name cannot be empty.");
            }
            n
        }
        None if interactive => {
            let mut prompt = Text::new("Profile name:")
                .with_help_message("A short identifier for this profile, e.g. 'prod' or 'staging'.")
                .with_validator(inquire::validator::MinLengthValidator::new(1));
            if is_first_profile {
                prompt = prompt.with_default("default");
            }
            prompt.prompt()?
        }
        // The profile name is the one trivial default allowed non-interactively.
        None => "default".to_string(),
    };

    if profile_file(&name)?.exists() && !force {
        if interactive {
            let confirmed = Confirm::new(&format!("Reconfigure profile '{name}'?"))
                .with_default(false)
                .with_help_message("The profile already exists; reconfiguring replaces it.")
                .prompt()?;
            if !confirmed {
                println!("Cancelled.");
                return Ok(());
            }
        } else {
            anyhow::bail!("profile '{name}' already exists — pass --force to overwrite");
        }
    }

    println!("Configuring profile '{name}'\n");

    let (mut profile, storage_desc) = if oauth {
        // `--oauth` answers the auth-method question; go straight to the
        // browser login. A terminal is not required: the approval happens in
        // the browser, the login URL is printed for the user (or an agent) to
        // open, and the remaining questions fall back to defaults.
        configure_oauth(&name, region_choice, interactive).await?
    } else if !interactive {
        build_profile_non_interactive(&name, api_key, region_choice)?
    } else if api_key.is_some() {
        // An API key was supplied, so the auth-method question is answered.
        configure_api_key(&name, api_key, region_choice)?
    } else {
        let auth_choice = Select::new("Authentication method:", AUTH_METHODS.to_vec())
            .with_starting_cursor(0) // OAuth is the default
            .with_help_message(
                "OAuth opens your browser for secure login. \
                 API key lets you paste credentials directly \
                 (must be a Team Key or Personal Key - not a Send-Your-Data key).",
            )
            .prompt()?;

        if auth_choice.starts_with("OAuth") {
            configure_oauth(&name, region_choice, interactive).await?
        } else {
            configure_api_key(&name, None, region_choice)?
        }
    };

    // ── Common: default output format (per-profile) ────────────────────────────
    // Non-interactive runs leave it unset, falling back to the global default.
    if interactive {
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
            "toon" => OutputFormat::Toon,
            _ => OutputFormat::Text,
        });
    }

    save_profile(&name, &profile)?;

    // On first profile creation, configure global safety settings and auto-set
    // as default. Non-interactive runs keep the (permissive) defaults.
    if is_first_profile {
        let mut global_config = load_config().unwrap_or_default();

        if interactive {
            println!("\n─── Global Safety Settings ───");
            println!("These apply to all profiles. Change later in ~/.cx/config.toml\n");

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
        }

        global_config.default_profile = name.clone();
        save_config(&global_config)?;
    }

    if !is_first_profile {
        let mut global_config = load_config().unwrap_or_default();
        let should_set_default = if set_default {
            true
        } else if interactive {
            Confirm::new(&format!("Set '{name}' as the default profile?"))
                .with_default(false)
                .prompt()?
        } else {
            false
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

// ── Non-interactive configure path ────────────────────────────────────────────

/// Build an API-key profile from flags alone — no terminal involved. OAuth is
/// unavailable here (it needs a browser), and consequential values are never
/// invented: a missing API key or region is an error naming the exact flag to
/// add. Credentials go to the profile file (the OS store may itself prompt).
fn build_profile_non_interactive(
    name: &str,
    api_key: Option<String>,
    region_choice: Option<RegionChoice>,
) -> Result<(Profile, &'static str)> {
    let mut missing = Vec::new();
    if region_choice.is_none() {
        missing.push("no region — pass --url or --region");
    }
    if api_key.is_none() {
        missing.push("no API key — pass --api-key or set CX_API_KEY");
    }
    if !missing.is_empty() {
        anyhow::bail!("{}", missing.join("\n"));
    }
    let (api_key, region_choice) = (api_key.unwrap(), region_choice.unwrap());

    let region = match region_choice {
        RegionChoice::Known(region) => region,
        RegionChoice::Custom { base_url } => Region::Custom(base_url),
    };

    // Clean up any leftover keychain entries from a previous config.
    keyring_store::delete_profile(name);
    let profile = Profile {
        auth: AuthKind::ApiKey,
        credential_storage: CredentialStorage::File,
        api_key: Some(api_key),
        region,
        label: None,
        oauth_client_id: None,
        oauth_base_url: None,
        oauth_tokens: None,
        default_output_format: None,
        default_tier: None,
        console_url: None,
        cached_console_url: None,
        cached_console_url_at: None,
    };
    Ok((profile, "profile file"))
}

// ── OAuth configure path ──────────────────────────────────────────────────────

async fn configure_oauth(
    name: &str,
    region_choice: Option<RegionChoice>,
    interactive: bool,
) -> Result<(Profile, &'static str)> {
    // Region / environment selection: the --url/--region flags when supplied,
    // otherwise the searchable list (URL derivation, BYOC). The browser login
    // itself needs no terminal — the user approves in the browser and the
    // callback lands on localhost — so without one, only the questions with
    // no flag answer are errors and the rest fall back to defaults.
    let region_choice = match region_choice {
        Some(choice) => choice,
        None if interactive => select_region_interactive()?,
        None => anyhow::bail!("no region — pass --url or --region"),
    };
    let is_custom;
    let (region, base_url, client_id, oauth_client_id_for_profile) = match region_choice {
        RegionChoice::Custom { base_url } => {
            is_custom = true;
            if !interactive {
                anyhow::bail!(
                    "a custom endpoint needs an OAuth client ID, which is prompted \
                     interactively — run on a terminal, or use --api-key/CX_API_KEY"
                );
            }
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

    let label = if interactive {
        Text::new("Label (e.g. 'prod'):")
            .prompt_skippable()?
            .filter(|s| !s.is_empty())
    } else {
        None
    };

    // Clean up any existing keyring entries before writing new ones, regardless
    // of which storage backend the user picks below.
    keyring_store::delete_profile(name);

    // ── Browser login ──────────────────────────────────────────────────────────
    println!();
    let tokens = oauth::browser_login(&base_url, &client_id).await?;
    println!("Login successful!");

    // Non-interactive runs store tokens in the profile file, matching the
    // non-interactive API-key path (the OS store may itself prompt).
    let credential_storage = if interactive {
        select_credential_storage(
            "Where should OAuth tokens be stored?",
            "'file' stores tokens in the profile config (0600 perms). \
             'os-store' uses the OS credential store (macOS Keychain, Windows Credential Manager).",
        )?
    } else {
        CredentialStorage::File
    };

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
        console_url: None,
        cached_console_url: None,
        cached_console_url_at: None,
    };

    Ok((profile, storage_desc))
}

// ── API key configure path ────────────────────────────────────────────────────

fn configure_api_key(
    name: &str,
    api_key: Option<String>,
    region_choice: Option<RegionChoice>,
) -> Result<(Profile, &'static str)> {
    let api_key = match api_key {
        Some(key) => key,
        None => {
            println!(
                "The API key must be a Team Key or a Personal Key.\n  \
                 • Team Key:     Data Flow → API Keys → Team Keys\n  \
                 • Personal Key: User menu (top-right) → Personal Keys\n\
                 Send-Your-Data / ingress keys will NOT work for querying."
            );
            Password::new("Coralogix API key (Team Key or Personal Key):")
                .with_display_mode(PasswordDisplayMode::Masked)
                .without_confirmation()
                .with_validator(|input: &str| {
                    use inquire::validator::Validation;
                    if input.trim().is_empty() {
                        Ok(Validation::Invalid("API key cannot be empty".into()))
                    } else {
                        Ok(Validation::Valid)
                    }
                })
                .prompt()?
        }
    };

    let region_choice = match region_choice {
        Some(choice) => choice,
        None => select_region_interactive()?,
    };
    let region = match region_choice {
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
                console_url: None,
                cached_console_url: None,
                cached_console_url_at: None,
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
                console_url: None,
                cached_console_url: None,
                cached_console_url_at: None,
            };
            (profile, "profile file")
        }
    };

    Ok((profile, storage_desc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::ValueEnum;

    /// The interactive picker seeds its cursor by matching `OutputFormat::as_str()`
    /// against `OUTPUT_FORMATS`. If a canonical variant string is missing from the
    /// list, the cursor silently falls back to index 0 instead of the user's current
    /// setting (this regressed when `Agents`/`agents` was renamed to `Toon`/`toon`).
    #[test]
    fn every_output_format_is_selectable_in_picker() {
        for variant in OutputFormat::value_variants() {
            assert!(
                OUTPUT_FORMATS.contains(&variant.as_str()),
                "OUTPUT_FORMATS is missing canonical variant {:?}; picker cursor would \
                 not preselect it",
                variant.as_str(),
            );
        }
    }

    /// The default cursor is a raw index into `PICKER_REGIONS`; if the list is
    /// ever reordered or an entry is inserted before it, the picker would
    /// silently pre-select a different region.
    #[test]
    fn region_default_cursor_points_at_eu2() {
        assert_eq!(
            PICKER_REGIONS[REGION_DEFAULT_CURSOR],
            Region::Eu2,
            "REGION_DEFAULT_CURSOR must point at eu2 in PICKER_REGIONS"
        );
    }

    // ── --region / --url flag resolution ─────────────────────────────────────

    #[test]
    fn region_flag_parses_known_shortname() {
        let region = parse_region_flag("eu2").expect("eu2 is a known region");
        assert_eq!(region, Region::Eu2);
    }

    #[test]
    fn region_flag_is_case_insensitive() {
        let region = parse_region_flag("EU2").expect("region flag should be case-insensitive");
        assert_eq!(region, Region::Eu2);
    }

    /// `Region::from_str` maps unknown strings to `Region::Custom`; the flag
    /// parser must reject those instead of writing a garbage endpoint.
    #[test]
    fn region_flag_rejects_unknown_shortname() {
        let err = parse_region_flag("eu22").expect_err("eu22 is not a region");
        assert!(
            err.to_string().contains("unknown region 'eu22'"),
            "error should name the bad value, got: {err}"
        );
    }

    #[test]
    fn no_region_flags_resolve_to_none() {
        assert!(resolve_region_flags(None, None)
            .expect("no flags is valid")
            .is_none());
    }

    #[test]
    fn url_flag_derives_known_region() {
        let choice = resolve_region_flags(Some("https://myteam.app.eu2.coralogix.com"), None)
            .expect("recognizable URL")
            .expect("URL flag should produce a choice");
        assert!(
            matches!(choice, RegionChoice::Known(Region::Eu2)),
            "app URL should derive eu2"
        );
    }

    /// A URL that doesn't map to a known Coralogix domain (BYOC / private
    /// link) is used verbatim as a custom endpoint, trailing slash stripped.
    #[test]
    fn url_flag_unresolved_becomes_custom_endpoint() {
        let choice = resolve_region_flags(Some("https://api.myenv.internal/"), None)
            .expect("host-bearing URL")
            .expect("URL flag should produce a choice");
        match choice {
            RegionChoice::Custom { base_url } => assert_eq!(base_url, "https://api.myenv.internal"),
            RegionChoice::Known(_) => panic!("unknown host must not resolve to a known region"),
        }
    }

    #[test]
    fn url_flag_without_host_is_rejected() {
        let err = resolve_region_flags(Some(""), None).expect_err("empty URL has no host");
        assert!(
            err.to_string().contains("not a valid URL"),
            "error should say the URL is invalid, got: {err}"
        );
    }

    /// A customer-supplied `--url` is stored exactly as typed (modulo trim and
    /// trailing slash) — we never rewrite it, e.g. by inventing an `https://`
    /// scheme the customer didn't ask for.
    #[test]
    fn url_flag_schemeless_host_is_stored_verbatim() {
        let choice = resolve_region_flags(Some("api.myenv.internal"), None)
            .expect("host-bearing input")
            .expect("URL flag should produce a choice");
        match choice {
            RegionChoice::Custom { base_url } => {
                assert_eq!(base_url, "api.myenv.internal")
            }
            RegionChoice::Known(region) => {
                panic!("unknown host must not resolve to a known region {region}")
            }
        }
    }

    /// A `--url` for a recognised but unlisted region (staging `stg1`, no OAuth
    /// client ID and no picker row) must become a custom endpoint instead of a
    /// Known region the OAuth flow can't service — matching the interactive flow.
    #[test]
    fn url_flag_unlisted_region_becomes_custom_endpoint() {
        let choice = resolve_region_flags(Some("https://team.app.stg1.coralogix.net"), None)
            .expect("host-bearing URL")
            .expect("URL flag should produce a choice");
        match choice {
            RegionChoice::Custom { base_url } => {
                assert_eq!(base_url, "https://team.app.stg1.coralogix.net")
            }
            RegionChoice::Known(region) => {
                panic!("expected custom endpoint, got known region {region}")
            }
        }
    }

    /// Every picker region must have an app URL template and render as
    /// `({short-name}) {template}`. A missing template would show a bare
    /// short-name and break URL-paste scoring against the label.
    #[test]
    fn every_region_option_is_short_name_and_url_template() {
        for region in PICKER_REGIONS {
            let template = region
                .app_url_template()
                .expect("picker region must have an app URL template");
            assert_eq!(
                region_option_label(region),
                format!("({region}) {template}")
            );
            assert!(
                !matches!(region, Region::Custom(_)),
                "PICKER_REGIONS entry {region} is not a known region"
            );
        }
    }

    #[test]
    fn region_picker_ends_with_custom_endpoint() {
        let options = region_picker_options();
        assert_eq!(options[options.len() - 1], CUSTOM_ENDPOINT_OPTION);
        assert_eq!(options.len(), PICKER_REGIONS.len() + 1);
    }

    /// A scheme-less endpoint must be stored with an `https://` scheme, otherwise
    /// `CxClient` and OAuth discovery build invalid request URLs from it.
    #[test]
    fn custom_base_url_adds_scheme_when_missing() {
        assert_eq!(
            custom_base_url_from("api.myenv.example.com").as_deref(),
            Some("https://api.myenv.example.com")
        );
    }

    /// An explicit scheme is preserved (http stays http), and a trailing slash
    /// is stripped.
    #[test]
    fn custom_base_url_preserves_scheme_and_strips_slash() {
        assert_eq!(
            custom_base_url_from("http://api.myenv.example.com/").as_deref(),
            Some("http://api.myenv.example.com")
        );
    }

    /// Input with no parseable host yields `None` rather than a bogus endpoint.
    #[test]
    fn custom_base_url_rejects_hostless_input() {
        assert_eq!(custom_base_url_from(""), None);
        assert_eq!(custom_base_url_from("   "), None);
    }

    /// A pasted URL for a listed region resolves to that region.
    #[test]
    fn paste_url_for_listed_region_stays_known() {
        match resolve_url_choice("https://team.app.eu2.coralogix.com").unwrap() {
            RegionChoice::Known(region) => assert_eq!(region.to_string(), "eu2"),
            RegionChoice::Custom { base_url } => {
                panic!("expected known region eu2, got custom endpoint {base_url}")
            }
        }
    }

    /// A recognised but unlisted region (staging `stg1`, no OAuth client ID and
    /// no picker row) must route to a custom endpoint instead of a Known region
    /// that the OAuth flow can't service.
    #[test]
    fn paste_url_for_unlisted_region_becomes_custom_endpoint() {
        match resolve_url_choice("https://team.app.stg1.coralogix.net").unwrap() {
            RegionChoice::Custom { base_url } => {
                assert_eq!(base_url, "https://team.app.stg1.coralogix.net")
            }
            RegionChoice::Known(region) => {
                panic!("expected custom endpoint, got known region {region}")
            }
        }
    }
}
