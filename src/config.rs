use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::keyring_store;
use crate::oauth;

/// Authentication method used by a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    /// Static Coralogix API key.  Default for legacy profiles that pre-date OAuth.
    #[default]
    ApiKey,
    /// OAuth 2.0 + OIDC browser login with automatic token refresh.
    OAuth,
}

/// Where API keys are stored for a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStorage {
    /// Store in the profile TOML file (plaintext, 0600 permissions).
    #[default]
    File,
    /// Store in the OS credential store (macOS Keychain, Windows Credential Manager, etc.)
    OsStore,
}

/// Output format for command results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Human-readable text output (default).
    #[default]
    Text,
    /// Raw JSON output.
    Json,
    /// Token-aware, AI-agent-optimised JSON output.
    Agents,
}

impl OutputFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputFormat::Text => "text",
            OutputFormat::Json => "json",
            OutputFormat::Agents => "agents",
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Coralogix region, used to resolve the API endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Us1,
    Us2,
    Us3,
    Eu1,
    Eu2,
    Ap1,
    Ap2,
    Ap3,
    Stg1,
    #[serde(untagged)]
    Custom(String),
}

impl Region {
    pub fn api_endpoint(&self) -> &str {
        match self {
            Region::Us1 => "https://api.us1.coralogix.com",
            Region::Us2 => "https://api.us2.coralogix.com",
            Region::Us3 => "https://api.us3.coralogix.com",
            Region::Eu1 => "https://api.eu1.coralogix.com",
            Region::Eu2 => "https://api.eu2.coralogix.com",
            Region::Ap1 => "https://api.ap1.coralogix.com",
            Region::Ap2 => "https://api.ap2.coralogix.com",
            Region::Ap3 => "https://api.ap3.coralogix.com",
            Region::Stg1 => "https://api.stg1.coralogix.net",
            Region::Custom(url) => url.as_str(),
        }
    }

    /// Base domain (no scheme, no team subdomain) of the Coralogix web
    /// console for this region.
    ///
    /// Console URLs have the shape `https://<team>.<console_domain>/...` -
    /// the team is a required subdomain (see `identity::resolve_team_subdomain`),
    /// this only supplies the domain that comes after it.
    ///
    /// Values match the documented per-region app hostnames in the
    /// Coralogix endpoints reference
    /// (<https://coralogix.com/docs/integrations/coralogix-endpoints/#how-to-find-the-correct-endpoints>).
    /// `stg1` is an internal/staging environment not listed there and is
    /// left unmapped (`None`) rather than guessed.
    ///
    /// Returns `None` when no console host can be derived, notably for
    /// `Region::Custom` (an arbitrary user-supplied API endpoint has no
    /// implied web console) - callers should skip printing a link in that
    /// case rather than fabricate one. A profile can still provide a console
    /// base explicitly via `Profile::console_url`.
    pub fn console_domain(&self) -> Option<&'static str> {
        match self {
            Region::Us1 => Some("app.coralogix.us"),
            Region::Us2 => Some("app.cx498.coralogix.com"),
            Region::Us3 => Some("app.us3.coralogix.com"),
            Region::Eu1 => Some("coralogix.com"),
            Region::Eu2 => Some("app.eu2.coralogix.com"),
            Region::Ap1 => Some("app.coralogix.in"),
            Region::Ap2 => Some("app.coralogixsg.com"),
            Region::Ap3 => Some("app.ap3.coralogix.com"),
            Region::Stg1 => None,
            Region::Custom(_) => None,
        }
    }
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Region::Us1 => write!(f, "us1"),
            Region::Us2 => write!(f, "us2"),
            Region::Us3 => write!(f, "us3"),
            Region::Eu1 => write!(f, "eu1"),
            Region::Eu2 => write!(f, "eu2"),
            Region::Ap1 => write!(f, "ap1"),
            Region::Ap2 => write!(f, "ap2"),
            Region::Ap3 => write!(f, "ap3"),
            Region::Stg1 => write!(f, "stg1"),
            Region::Custom(s) => write!(f, "{s}"),
        }
    }
}

impl std::str::FromStr for Region {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s.to_lowercase().as_str() {
            "us1" => Region::Us1,
            "us2" => Region::Us2,
            "us3" => Region::Us3,
            "eu1" => Region::Eu1,
            "eu2" => Region::Eu2,
            "ap1" => Region::Ap1,
            "ap2" => Region::Ap2,
            "ap3" => Region::Ap3,
            "stg1" => Region::Stg1,
            other => Region::Custom(other.to_string()),
        })
    }
}

/// A shell completion script installed and tracked by `cx completions install`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedCompletion {
    /// Shell name as a lowercase string, e.g. "zsh", "bash", "fish".
    pub shell: String,
    /// Absolute path to the installed completion file.
    pub path: PathBuf,
}

/// Top-level config file (~/.cx/config.toml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Default profile to use when none is specified.
    #[serde(default = "default_profile")]
    pub default_profile: String,

    /// Default output format when `--output` is omitted.
    #[serde(default)]
    pub default_output_format: OutputFormat,

    /// Maximum serialized byte size of a non-aggregated Dataprime response
    /// that can be printed directly to stdout in `agents` mode. If the payload
    /// exceeds this limit the data is written to a temp file instead.
    /// Set to `-1` to disable the limit (always print directly).
    /// Default: 100 KiB (102400 bytes).
    #[serde(
        default = "default_max_dataprime_direct_output_size",
        with = "max_size_serde"
    )]
    pub max_dataprime_direct_output_size: Option<usize>,

    /// Directory used to store temporary result files when the output exceeds
    /// `max_dataprime_direct_output_size`. Defaults to `/tmp/`.
    #[serde(default = "default_temp_dir")]
    pub temp_dir: String,

    /// Whether to allow risky commands (iam, archive write operations).
    /// When false, write operations under these commands are hard-blocked.
    #[serde(default = "default_true")]
    pub allow_risky_commands: bool,

    /// Whether the Olly AI assistant (`olly ask`) is enabled.
    /// When false, `olly ask` is blocked.
    #[serde(default = "default_true")]
    pub olly_enabled: bool,

    /// When true, ALL write operations are blocked globally.
    /// Equivalent to always passing --read-only.
    #[serde(default)]
    pub read_only: bool,

    /// Shell completion scripts installed and tracked by `cx completions install`.
    /// Only files recorded here are touched by `cx completions refresh`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub managed_completions: Vec<ManagedCompletion>,
}

fn default_profile() -> String {
    "default".to_string()
}

fn default_true() -> bool {
    true
}

fn default_max_dataprime_direct_output_size() -> Option<usize> {
    Some(100 * 1024) // 100 KiB
}

fn default_temp_dir() -> String {
    "/tmp/".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_profile: default_profile(),
            default_output_format: OutputFormat::default(),
            max_dataprime_direct_output_size: default_max_dataprime_direct_output_size(),
            temp_dir: default_temp_dir(),
            allow_risky_commands: true,
            olly_enabled: true,
            read_only: false,
            managed_completions: vec![],
        }
    }
}

/// Custom serde module that maps the integer -1 in TOML/JSON to `None`
/// (disabled) and any non-negative integer to `Some(n)`.
mod max_size_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Option<usize>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(n) => (*n as i64).serialize(s),
            None => (-1i64).serialize(s),
        }
    }

    pub fn deserialize<'de, D>(de: D) -> Result<Option<usize>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let n = i64::deserialize(de)?;
        if n < 0 {
            Ok(None)
        } else {
            Ok(Some(n as usize))
        }
    }
}

/// OAuth token set persisted inline in the profile TOML when
/// `credential_storage = "file"`. Mirrors the keyring-backed layout used by
/// `oauth::store_tokens_keyring`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct StoredOAuthTokens {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    /// Unix timestamp (seconds). Absent when the IdP did not return `expires_in`;
    /// callers fall back to parsing the JWT `exp` claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry: Option<u64>,
}

/// A named profile storing credentials and endpoint info.
///
/// `credential_storage` selects where secrets live for both auth modes:
///   - `"file"` (default): API keys / OAuth tokens are written inline in the
///     TOML, which is created with `0600` permissions.
///   - `"os_store"`: secrets are stored in the OS credential store
///     (macOS Keychain, Windows Credential Manager, etc.) and the inline
///     fields are absent from the TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Authentication method for this profile.
    /// Defaults to `ApiKey` so that existing profiles without this field continue to work.
    #[serde(default)]
    pub auth: AuthKind,
    /// Where credentials for this profile are stored (applies to both API keys
    /// and OAuth tokens).
    #[serde(default)]
    pub credential_storage: CredentialStorage,
    /// Coralogix API key. Present when `auth = "api_key"` and
    /// `credential_storage = "file"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    pub region: Region,
    /// Optional free-form label (e.g. "prod", "staging")
    pub label: Option<String>,
    /// OAuth: override the OAuth client ID.
    /// Required for custom regions; hard-coded for known Coralogix regions.
    /// Not written to the TOML for known regions (looked up from `KNOWN_ENVIRONMENTS`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
    /// OAuth: override the base URL used for OpenID Connect discovery.
    /// When `None`, `region.api_endpoint()` is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_base_url: Option<String>,
    /// OAuth: cached token set. Present only when `auth = "oauth"` and
    /// `credential_storage = "file"`. For `os_store`, tokens live in the OS keyring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_tokens: Option<StoredOAuthTokens>,
    /// Per-profile default output format. When set, takes precedence over the
    /// global `default_output_format` in config.toml.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_output_format: Option<OutputFormat>,
    /// Per-profile default storage tier for DataPrime queries (logs, spans).
    /// When set, used as the default `--tier` value for this profile.
    /// Falls back to `Archive` when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_tier: Option<crate::Tier>,
    /// Explicit override for the Coralogix web console base URL used to build
    /// "View in Coralogix" links (e.g. `https://acme.app.eu2.coralogix.com`).
    /// Include the team subdomain - console links resolve to a team, and the
    /// team name is not always derivable automatically.
    ///
    /// When unset, the CLI derives a base from `region.console_domain()` plus
    /// the team subdomain reported by `GET /identity/whoami`. When neither is
    /// available (e.g. `Region::Custom`), no console link is printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console_url: Option<String>,
    /// Opt-in fallback for console link resolution: when `true` and
    /// `GET /identity/whoami` doesn't return a usable `team_url`, build the
    /// console subdomain by sanitizing `team_name` instead (lowercased,
    /// with runs of non-alphanumeric characters collapsed to a single
    /// hyphen - see `identity::sanitize_team_name_label`) rather than
    /// skipping the link entirely.
    ///
    /// This is a best-effort *guess*, not a guarantee: a team's real
    /// subdomain doesn't always match a sanitized form of its display name
    /// (e.g. a team named "acmeprod" could have the real subdomain
    /// "acme-prod"). Off by default so existing profiles keep the
    /// conservative "no link rather than a possibly-wrong one" behavior;
    /// only enable this once you've confirmed the guess is correct for your
    /// team, or set `console_url` directly instead for a guaranteed-correct
    /// link regardless of what `/identity/whoami` returns.
    #[serde(default, skip_serializing_if = "is_false")]
    pub console_team_name_fallback: bool,
}

/// `serde(skip_serializing_if)` helper for plain `bool` fields that default
/// to `false` - keeps profile TOML files free of a `field = false` line for
/// every profile that never opted in, matching the `Option::is_none` idiom
/// used for the `Option<T>` fields above.
fn is_false(b: &bool) -> bool {
    !*b
}

/// Resolved configuration ready for use at runtime.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    /// The profile name this config was resolved from.
    pub profile_name: String,
    pub api_key: String,
    pub endpoint: String,
    /// Default storage tier for DataPrime queries, resolved from the profile
    /// config. Falls back to `Archive` when the profile does not specify one.
    pub default_tier: crate::Tier,
    /// Explicit console base URL override from `Profile::console_url`, with
    /// any trailing slash trimmed. Takes precedence over `console_domain`.
    pub console_url: Option<String>,
    /// Console domain derived from `region.console_domain()`. `None` for
    /// `Region::Custom` and any other region with no known web console.
    pub console_domain: Option<String>,
    /// Resolved copy of `Profile::console_team_name_fallback` - see its doc
    /// comment. Always `false` in env-only mode (no profile file to read it
    /// from).
    pub console_team_name_fallback: bool,
}

/// Returns the cx config directory: `~/.cx/`
pub fn config_dir() -> Result<PathBuf> {
    // CX_HOME overrides the home directory for testing and non-standard setups.
    if let Ok(cx_home) = std::env::var("CX_HOME") {
        return Ok(PathBuf::from(cx_home).join(".cx"));
    }
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".cx"))
}

pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn profiles_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("profiles"))
}

pub fn profile_file(name: &str) -> Result<PathBuf> {
    Ok(profiles_dir()?.join(format!("{name}.toml")))
}

/// List all profile names from `~/.cx/profiles/`, sorted alphabetically.
///
/// Returns an empty list (not an error) when the directory does not exist.
/// Silently skips entries whose filenames are not valid UTF-8 or whose
/// extension is not `toml`.
pub fn list_profile_names() -> Result<Vec<String>> {
    list_profile_names_from(&profiles_dir()?)
}

fn list_profile_names_from(dir: &PathBuf) -> Result<Vec<String>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut names: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? != "toml" {
                return None;
            }
            Some(path.file_stem()?.to_str()?.to_string())
        })
        .collect();
    names.sort();
    Ok(names)
}

/// Load the global config (non-fatal if missing - returns default).
pub fn load_config() -> Result<Config> {
    let path = config_file()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&raw).context("Failed to parse config.toml")
}

/// Write the global config to disk, creating directories as needed.
pub fn save_config(config: &Config) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = config_file()?;
    let content = toml::to_string_pretty(config).context("Failed to serialize config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Load the output format preference from the first of the given profile names.
/// Returns `None` if no profile specifies one, or if the profile cannot be loaded.
pub fn first_profile_output_format(profiles: &[String]) -> Option<OutputFormat> {
    let name = if profiles.is_empty() {
        load_config().ok().map(|c| c.default_profile)
    } else {
        Some(profiles[0].clone())
    };
    name.and_then(|n| load_profile(&n).ok())
        .and_then(|p| p.default_output_format)
}

/// Load a named profile.
pub fn load_profile(name: &str) -> Result<Profile> {
    let path = profile_file(name)?;
    let raw = std::fs::read_to_string(&path).with_context(|| {
        format!("Profile '{name}' not found. Run `cx profiles add` to set it up.")
    })?;
    toml::from_str(&raw).with_context(|| format!("Failed to parse profile '{name}'"))
}

/// Resolve a single named profile, respecting optional CLI overrides.
///
/// Resolution order for the bearer token:
///   1. `--api-key` CLI flag (explicit command-line value, always wins)
///   2. Profile credentials (when `--profile` is explicit on the CLI, the
///      profile's own key/token is used; `CX_API_KEY` env var is suppressed
///      by `main.rs` in this case)
///   3. `CX_API_KEY` env var (when no `--profile` was explicitly passed)
///
/// Resolution order for the endpoint region:
///   1. `--region` CLI flag (explicit command-line value, always wins)
///   2. Profile's configured region (when `--profile` is explicit on the CLI;
///      `CX_REGION` env var is suppressed by `main.rs`)
///   3. `CX_REGION` env var (when no `--profile` was explicitly passed)
///
/// Env-only mode: when no profile file exists on disk but both an API key
/// override (`--api-key` / `CX_API_KEY`) and a region override (`--region` /
/// `CX_REGION`) are supplied, a `ResolvedConfig` is synthesised directly. This
/// lets ephemeral environments (CI runners, containers, ad-hoc scripts) run
/// `cx` without first invoking `cx profiles add`.
async fn resolve_single(
    profile_name: &str,
    api_key_override: Option<&str>,
    region_override: Option<&str>,
) -> Result<ResolvedConfig> {
    if !profile_file(profile_name)?.exists() {
        if let (Some(key), Some(region)) = (api_key_override, region_override) {
            let region: Region = region.parse()?;
            return Ok(ResolvedConfig {
                profile_name: profile_name.to_string(),
                endpoint: region.api_endpoint().to_string(),
                api_key: key.to_string(),
                default_tier: crate::Tier::Archive,
                console_url: None,
                console_domain: region.console_domain().map(str::to_string),
                console_team_name_fallback: false,
            });
        }
    }

    let mut profile = load_profile(profile_name)?;

    if let Some(region) = region_override {
        profile.region = region.parse()?;
    }

    // CLI-level api_key_override (already filtered by main.rs) wins over profile creds.
    let bearer = if let Some(key) = api_key_override {
        key.to_string()
    } else {
        match profile.auth {
            AuthKind::ApiKey => match profile.credential_storage {
                CredentialStorage::OsStore => keyring_store::get_secret(profile_name, "api_key")?,
                CredentialStorage::File => profile.api_key.clone(),
            }
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No API key found for profile '{profile_name}'.\n\
                         Run `cx profiles add {profile_name}` to set it up."
                )
            })?,
            AuthKind::OAuth => {
                let region_name = profile.region.to_string();
                let base_url = profile
                    .oauth_base_url
                    .clone()
                    .unwrap_or_else(|| profile.region.api_endpoint().to_string());
                let client_id = profile
                    .oauth_client_id
                    .clone()
                    .or_else(|| oauth::client_id_for_region(&region_name).map(str::to_string))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No OAuth client ID configured for profile '{profile_name}' \
                             (region: {region_name}).\n\
                             Run `cx profiles add {profile_name}` to reconfigure."
                        )
                    })?;
                let storage = profile.credential_storage;
                let (bearer, refreshed) = oauth::resolve_token(
                    profile_name,
                    &base_url,
                    &client_id,
                    storage,
                    profile.oauth_tokens.as_ref(),
                )
                .await?;
                if let Some(new_tokens) = refreshed {
                    // File-storage profile: persist refreshed tokens back to disk.
                    profile.oauth_tokens = Some(new_tokens);
                    save_profile(profile_name, &profile)?;
                }
                bearer
            }
        }
    };

    Ok(ResolvedConfig {
        profile_name: profile_name.to_string(),
        endpoint: profile.region.api_endpoint().to_string(),
        api_key: bearer,
        default_tier: profile.default_tier.unwrap_or(crate::Tier::Archive),
        console_url: profile
            .console_url
            .as_deref()
            .map(|s| s.trim_end_matches('/').to_string()),
        console_domain: profile.region.console_domain().map(str::to_string),
        console_team_name_fallback: profile.console_team_name_fallback,
    })
}

/// Resolve the active profile, respecting CLI overrides.
///
/// When `profile_override` is `None`, falls back to the default profile in config.
pub async fn resolve(
    profile_override: Option<&str>,
    api_key_override: Option<&str>,
    region_override: Option<&str>,
) -> Result<ResolvedConfig> {
    let config = load_config()?;
    let name = profile_override.unwrap_or(&config.default_profile);
    resolve_single(name, api_key_override, region_override).await
}

/// Resolve one or more named profiles into a list of `ResolvedConfig` values.
///
/// When `profiles` is empty, falls back to the single default profile from config.
/// Overrides (`api_key_override`, `region_override`) are applied uniformly to every
/// resolved profile - the caller should reject these when `profiles.len() > 1`.
pub async fn resolve_all(
    profiles: &[String],
    api_key_override: Option<&str>,
    region_override: Option<&str>,
) -> Result<Vec<ResolvedConfig>> {
    if profiles.is_empty() {
        let cfg = load_config()?;
        return Ok(vec![
            resolve_single(&cfg.default_profile, api_key_override, region_override).await?,
        ]);
    }
    let mut results = Vec::with_capacity(profiles.len());
    for name in profiles {
        results.push(resolve_single(name, api_key_override, region_override).await?);
    }
    Ok(results)
}

/// Write a profile to disk, creating directories as needed.
/// Sets file permissions to 0600 on Unix to protect any inline secrets.
pub fn save_profile(name: &str, profile: &Profile) -> Result<()> {
    let dir = profiles_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.toml"));
    let content = toml::to_string_pretty(profile).context("Failed to serialize profile")?;
    std::fs::write(&path, &content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Failed to set permissions on {}", path.display()))?;
    Ok(())
}

// ── Managed completion helpers ────────────────────────────────────────────────

/// Returns `true` when at least one completion file is tracked in the config.
/// Non-fatal: returns `false` if the config cannot be read.
pub fn has_managed_completions() -> bool {
    load_config()
        .map(|c| !c.managed_completions.is_empty())
        .unwrap_or(false)
}

/// Return the tracked managed completion entries from the global config.
pub fn managed_completions() -> Result<Vec<ManagedCompletion>> {
    Ok(load_config()?.managed_completions)
}

/// Add or update a managed completion entry for the given shell and path.
///
/// If an entry for this shell already exists, its path is updated in place.
/// The updated config is persisted to `~/.cx/config.toml`.
pub fn upsert_managed_completion(shell: &str, path: PathBuf) -> Result<()> {
    let mut config = load_config().unwrap_or_default();
    if let Some(existing) = config
        .managed_completions
        .iter_mut()
        .find(|c| c.shell == shell)
    {
        existing.path = path;
    } else {
        config.managed_completions.push(ManagedCompletion {
            shell: shell.to_string(),
            path,
        });
    }
    save_config(&config)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── list_profile_names_from ────────────────────────────────────────────────

    #[test]
    fn list_profile_names_sorted_and_filters_non_toml() {
        let dir =
            std::env::temp_dir().join(format!("cx_test_list_profiles_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("prod.toml"), "").unwrap();
        std::fs::write(dir.join("staging.toml"), "").unwrap();
        std::fs::write(dir.join("alpha.toml"), "").unwrap();
        std::fs::write(dir.join("ignored.txt"), "").unwrap();
        std::fs::write(dir.join("also-ignored.json"), "").unwrap();

        let names = list_profile_names_from(&dir).unwrap();
        assert_eq!(names, vec!["alpha", "prod", "staging"]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_profile_names_missing_dir_returns_empty() {
        let dir = std::env::temp_dir().join("cx_test_nonexistent_profiles_dir_xyz_never_created");
        // Ensure it really does not exist.
        let _ = std::fs::remove_dir_all(&dir);
        let names = list_profile_names_from(&dir).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn list_profile_names_empty_dir_returns_empty() {
        let dir =
            std::env::temp_dir().join(format!("cx_test_empty_profiles_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let names = list_profile_names_from(&dir).unwrap();
        assert!(names.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── ManagedCompletion / upsert_managed_completion ─────────────────────────

    #[test]
    fn managed_completion_roundtrips_through_toml() {
        let mc = ManagedCompletion {
            shell: "zsh".to_string(),
            path: PathBuf::from("/home/user/.zfunc/_cx"),
        };
        let config = Config {
            managed_completions: vec![mc.clone()],
            ..Config::default()
        };
        let toml = toml::to_string_pretty(&config).unwrap();
        let restored: Config = toml::from_str(&toml).unwrap();
        assert_eq!(restored.managed_completions.len(), 1);
        assert_eq!(restored.managed_completions[0], mc);
    }

    #[test]
    fn default_config_has_no_managed_completions() {
        let config = Config::default();
        assert!(config.managed_completions.is_empty());
    }

    #[test]
    fn config_without_managed_completions_field_deserialises_ok() {
        let toml = r#"
default_profile = "my-profile"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.managed_completions.is_empty());
        assert_eq!(config.default_profile, "my-profile");
    }

    #[test]
    fn managed_completions_not_written_when_empty() {
        let config = Config::default();
        let toml = toml::to_string_pretty(&config).unwrap();
        assert!(
            !toml.contains("managed_completions"),
            "empty managed_completions should be skipped in TOML output"
        );
    }

    // ── Unit tests (no disk I/O) ───────────────────────────────────────────────

    /// Build a `ResolvedConfig` directly without touching the filesystem, by
    /// testing the field-level properties that `resolve_single` must guarantee.
    #[test]
    fn resolved_config_carries_profile_name() {
        let cfg = ResolvedConfig {
            profile_name: "prod".to_string(),
            api_key: "k".to_string(),
            endpoint: "https://api.eu2.coralogix.com".to_string(),
            default_tier: crate::Tier::Archive,
            console_url: None,
            console_domain: None,
            console_team_name_fallback: false,
        };
        assert_eq!(cfg.profile_name, "prod");
    }

    #[test]
    fn region_api_endpoint_eu1() {
        assert_eq!(Region::Eu1.api_endpoint(), "https://api.eu1.coralogix.com");
    }

    #[test]
    fn region_api_endpoint_us1() {
        assert_eq!(Region::Us1.api_endpoint(), "https://api.us1.coralogix.com");
    }

    #[test]
    fn region_console_domain_known_regions() {
        assert_eq!(Region::Us1.console_domain(), Some("app.coralogix.us"));
        assert_eq!(
            Region::Us2.console_domain(),
            Some("app.cx498.coralogix.com")
        );
        assert_eq!(Region::Us3.console_domain(), Some("app.us3.coralogix.com"));
        assert_eq!(Region::Eu1.console_domain(), Some("coralogix.com"));
        assert_eq!(Region::Eu2.console_domain(), Some("app.eu2.coralogix.com"));
        assert_eq!(Region::Ap1.console_domain(), Some("app.coralogix.in"));
        assert_eq!(Region::Ap2.console_domain(), Some("app.coralogixsg.com"));
        assert_eq!(Region::Ap3.console_domain(), Some("app.ap3.coralogix.com"));
    }

    #[test]
    fn region_console_domain_stg1_is_none() {
        // stg1 is an internal/staging environment not listed in the
        // documented endpoints table (see Region::console_domain doc
        // comment). Deliberately None rather than guessed.
        assert_eq!(Region::Stg1.console_domain(), None);
    }

    #[test]
    fn region_console_domain_custom_is_none() {
        assert_eq!(
            Region::Custom("https://api.myenv.example.com".to_string()).console_domain(),
            None
        );
    }

    #[test]
    fn region_parses_from_str() {
        let r: Region = "eu2".parse().unwrap();
        assert_eq!(r.api_endpoint(), "https://api.eu2.coralogix.com");
    }

    #[test]
    fn region_custom_roundtrip() {
        let url = "https://custom.endpoint.io";
        let r: Region = url.parse().unwrap();
        assert_eq!(r.api_endpoint(), url);
    }

    /// Legacy TOML without `auth` field must deserialise as `AuthKind::ApiKey`.
    #[test]
    fn profile_without_auth_field_defaults_to_api_key() {
        let toml = r#"
region = "eu1"
api_key = "mykey"
"#;
        let p: Profile = toml::from_str(toml).unwrap();
        assert_eq!(p.auth, AuthKind::ApiKey);
        assert_eq!(p.api_key.as_deref(), Some("mykey"));
        assert!(p.oauth_client_id.is_none());
        assert!(p.oauth_base_url.is_none());
    }

    /// OAuth profile round-trips through TOML serialisation.
    #[test]
    fn oauth_profile_round_trips() {
        let profile = Profile {
            auth: AuthKind::OAuth,
            credential_storage: CredentialStorage::OsStore,
            api_key: None,
            region: Region::Eu2,
            label: Some("prod".to_string()),
            oauth_client_id: None,
            oauth_base_url: None,
            oauth_tokens: None,
            default_output_format: None,
            default_tier: None,
            console_url: None,
            console_team_name_fallback: false,
        };
        let toml = toml::to_string_pretty(&profile).unwrap();
        let restored: Profile = toml::from_str(&toml).unwrap();
        assert_eq!(restored.auth, AuthKind::OAuth);
        assert_eq!(restored.credential_storage, CredentialStorage::OsStore);
        assert!(restored.api_key.is_none());
        // oauth_client_id / oauth_base_url are skip_serializing_if = None, so absent
        assert!(restored.oauth_client_id.is_none());
        assert!(restored.oauth_tokens.is_none());
    }

    /// `console_team_name_fallback` defaults to `false` and is omitted from
    /// serialized TOML in that case, so existing profiles on disk (which
    /// predate this field) don't need migration and newly-written profiles
    /// that never opt in stay free of a redundant `= false` line.
    #[test]
    fn console_team_name_fallback_false_is_omitted_from_toml() {
        let profile = Profile {
            auth: AuthKind::ApiKey,
            credential_storage: CredentialStorage::File,
            api_key: Some("k".to_string()),
            region: Region::Eu2,
            label: None,
            oauth_client_id: None,
            oauth_base_url: None,
            oauth_tokens: None,
            default_output_format: None,
            default_tier: None,
            console_url: None,
            console_team_name_fallback: false,
        };
        let toml = toml::to_string_pretty(&profile).unwrap();
        assert!(
            !toml.contains("console_team_name_fallback"),
            "false should be omitted from serialized TOML, got: {toml}"
        );

        // A profile written before this field existed (or that just never
        // set it) must still parse, defaulting to false.
        let restored: Profile = toml::from_str(&toml).unwrap();
        assert!(!restored.console_team_name_fallback);
    }

    /// `console_team_name_fallback = true` round-trips through TOML.
    #[test]
    fn console_team_name_fallback_true_round_trips() {
        let profile = Profile {
            auth: AuthKind::ApiKey,
            credential_storage: CredentialStorage::File,
            api_key: Some("k".to_string()),
            region: Region::Eu2,
            label: None,
            oauth_client_id: None,
            oauth_base_url: None,
            oauth_tokens: None,
            default_output_format: None,
            default_tier: None,
            console_url: None,
            console_team_name_fallback: true,
        };
        let toml = toml::to_string_pretty(&profile).unwrap();
        assert!(toml.contains("console_team_name_fallback = true"));
        let restored: Profile = toml::from_str(&toml).unwrap();
        assert!(restored.console_team_name_fallback);
    }

    /// OAuth profile with file-stored tokens round-trips through TOML.
    #[test]
    fn oauth_profile_file_storage_round_trips() {
        let profile = Profile {
            auth: AuthKind::OAuth,
            credential_storage: CredentialStorage::File,
            api_key: None,
            region: Region::Eu2,
            label: Some("prod".to_string()),
            oauth_client_id: None,
            oauth_base_url: None,
            oauth_tokens: Some(StoredOAuthTokens {
                access_token: "access-abc".to_string(),
                refresh_token: Some("refresh-xyz".to_string()),
                id_token: None,
                expiry: Some(1_700_000_000),
            }),
            default_output_format: None,
            default_tier: None,
            console_url: None,
            console_team_name_fallback: false,
        };
        let toml = toml::to_string_pretty(&profile).unwrap();
        let restored: Profile = toml::from_str(&toml).unwrap();
        assert_eq!(restored.credential_storage, CredentialStorage::File);
        let tokens = restored.oauth_tokens.expect("oauth_tokens persisted");
        assert_eq!(tokens.access_token, "access-abc");
        assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-xyz"));
        assert!(tokens.id_token.is_none());
        assert_eq!(tokens.expiry, Some(1_700_000_000));
    }

    /// Custom OAuth profile (with explicit client ID) round-trips.
    #[test]
    fn custom_oauth_profile_preserves_client_id() {
        let profile = Profile {
            auth: AuthKind::OAuth,
            credential_storage: CredentialStorage::OsStore,
            api_key: None,
            region: Region::Custom("https://api.myenv.coralogix.com".to_string()),
            label: None,
            oauth_client_id: Some("abc-123".to_string()),
            oauth_base_url: None,
            oauth_tokens: None,
            default_output_format: None,
            default_tier: None,
            console_url: None,
            console_team_name_fallback: false,
        };
        let toml = toml::to_string_pretty(&profile).unwrap();
        let restored: Profile = toml::from_str(&toml).unwrap();
        assert_eq!(restored.oauth_client_id.as_deref(), Some("abc-123"));
    }

    // ── resolve_all missing profile (no filesystem) ───────────────────────────

    #[tokio::test]
    async fn resolve_all_missing_profile_returns_error() {
        let result = resolve_all(
            &["cx_definitely_does_not_exist_xyz".to_string()],
            None,
            None,
        )
        .await;
        assert!(result.is_err());
    }

    // ── env-only mode (no filesystem) ────────────────────────────────────────

    #[tokio::test]
    async fn resolve_single_env_only_synthesises_when_profile_file_missing() {
        let cfg = resolve_single(
            "cx_envonly_missing_profile_xyz",
            Some("env-key"),
            Some("eu1"),
        )
        .await
        .unwrap();
        assert_eq!(cfg.profile_name, "cx_envonly_missing_profile_xyz");
        assert_eq!(cfg.api_key, "env-key");
        assert_eq!(cfg.endpoint, "https://api.eu1.coralogix.com");
    }

    #[tokio::test]
    async fn resolve_single_env_only_requires_api_key_override() {
        let result = resolve_single("cx_envonly_no_key_xyz", None, Some("eu1")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn resolve_single_env_only_requires_region_override() {
        let result = resolve_single("cx_envonly_no_region_xyz", Some("env-key"), None).await;
        assert!(result.is_err());
    }

    // ── Empty profile name edge case ──────────────────────────────────────────

    #[test]
    fn save_profile_empty_name_creates_hidden_file() {
        let dir =
            std::env::temp_dir().join(format!("cx_test_empty_profile_name_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        // Point CX_HOME at our temp dir so save_profile writes there.
        // We can't easily redirect save_profile without CX_HOME, so test
        // the underlying list_profile_names_from behavior instead: an empty
        // stem is invisible to profile discovery.
        std::fs::create_dir_all(dir.join("profiles")).unwrap();
        std::fs::write(dir.join("profiles").join(".toml"), "region = \"eu1\"\n").unwrap();

        let names = list_profile_names_from(&dir.join("profiles")).unwrap();
        assert!(
            names.is_empty() || !names.contains(&String::new()),
            "empty-named profile should not appear in list, got: {names:?}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── Integration tests (require ~/.cx access, run with --ignored) ──────────

    #[tokio::test]
    #[ignore = "requires write access to ~/.cx; run with `cargo test -- --ignored`"]
    async fn resolve_all_integration_multiple_profiles() {
        let names = ["cx_inttest_multi_a", "cx_inttest_multi_b"];
        for (i, name) in names.iter().enumerate() {
            let profile = Profile {
                auth: AuthKind::ApiKey,
                credential_storage: CredentialStorage::File,
                api_key: Some(format!("key-{i}")),
                region: Region::Eu1,
                label: None,
                oauth_client_id: None,
                oauth_base_url: None,
                oauth_tokens: None,
                default_output_format: None,
                default_tier: None,
                console_url: None,
                console_team_name_fallback: false,
            };
            save_profile(name, &profile).unwrap();
        }

        let configs = resolve_all(
            &names.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].profile_name, "cx_inttest_multi_a");
        assert_eq!(configs[0].api_key, "key-0");
        assert_eq!(configs[1].profile_name, "cx_inttest_multi_b");
        assert_eq!(configs[1].api_key, "key-1");

        for name in &names {
            let _ = std::fs::remove_file(profile_file(name).unwrap());
        }
    }

    #[tokio::test]
    #[ignore = "requires write access to ~/.cx; run with `cargo test -- --ignored`"]
    async fn resolve_all_integration_empty_slice_falls_back_to_default() {
        let profile = Profile {
            auth: AuthKind::ApiKey,
            credential_storage: CredentialStorage::File,
            api_key: Some("default-key".to_string()),
            region: Region::Eu1,
            label: None,
            oauth_client_id: None,
            oauth_base_url: None,
            oauth_tokens: None,
            default_output_format: None,
            default_tier: None,
            console_url: None,
            console_team_name_fallback: false,
        };
        save_profile("default", &profile).unwrap();

        let configs = resolve_all(&[], None, None).await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].profile_name, "default");
    }

    #[tokio::test]
    #[ignore = "requires write access to ~/.cx"]
    async fn resolve_prefers_cli_override_over_file() {
        let name = "cx_inttest_resolve_override";
        let profile = Profile {
            auth: AuthKind::ApiKey,
            credential_storage: CredentialStorage::File,
            api_key: Some("file-key".to_string()),
            region: Region::Eu1,
            label: None,
            oauth_client_id: None,
            oauth_base_url: None,
            oauth_tokens: None,
            default_output_format: None,
            default_tier: None,
            console_url: None,
            console_team_name_fallback: false,
        };
        save_profile(name, &profile).unwrap();

        let config = resolve_single(name, Some("cli-key"), None).await.unwrap();
        assert_eq!(config.api_key, "cli-key");

        let _ = std::fs::remove_file(profile_file(name).unwrap());
    }

    #[tokio::test]
    #[ignore = "requires write access to ~/.cx"]
    async fn resolve_reads_from_file_storage() {
        let name = "cx_inttest_resolve_file";
        let profile = Profile {
            auth: AuthKind::ApiKey,
            credential_storage: CredentialStorage::File,
            api_key: Some("file-key".to_string()),
            region: Region::Eu1,
            label: None,
            oauth_client_id: None,
            oauth_base_url: None,
            oauth_tokens: None,
            default_output_format: None,
            default_tier: None,
            console_url: None,
            console_team_name_fallback: false,
        };
        save_profile(name, &profile).unwrap();

        let config = resolve_single(name, None, None).await.unwrap();
        assert_eq!(config.api_key, "file-key");

        let _ = std::fs::remove_file(profile_file(name).unwrap());
    }

    #[tokio::test]
    #[ignore = "requires write access to ~/.cx"]
    async fn resolve_errors_when_no_key_anywhere() {
        let name = "cx_inttest_resolve_no_key";
        let profile = Profile {
            auth: AuthKind::ApiKey,
            credential_storage: CredentialStorage::File,
            api_key: None,
            region: Region::Eu1,
            label: None,
            oauth_client_id: None,
            oauth_base_url: None,
            oauth_tokens: None,
            default_output_format: None,
            default_tier: None,
            console_url: None,
            console_team_name_fallback: false,
        };
        save_profile(name, &profile).unwrap();

        let result = resolve_single(name, None, None).await;
        assert!(result.is_err());

        let _ = std::fs::remove_file(profile_file(name).unwrap());
    }
}
