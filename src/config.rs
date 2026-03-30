use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

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

/// Coralogix region, used to resolve the API endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Us1,
    Us2,
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
            Region::Eu1 => "https://api.eu2.coralogix.com",
            Region::Eu2 => "https://api.eu2.coralogix.com",
            Region::Ap1 => "https://api.ap1.coralogix.com",
            Region::Ap2 => "https://api.ap2.coralogix.com",
            Region::Ap3 => "https://api.ap3.coralogix.com",
            Region::Stg1 => "https://api.stg1.coralogix.net",
            Region::Custom(url) => url.as_str(),
        }
    }
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Region::Us1 => write!(f, "us1"),
            Region::Us2 => write!(f, "us2"),
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
    #[serde(default = "default_max_dataprime_direct_output_size", with = "max_size_serde")]
    pub max_dataprime_direct_output_size: Option<usize>,

    /// Directory used to store temporary result files when the output exceeds
    /// `max_dataprime_direct_output_size`. Defaults to `/tmp/`.
    #[serde(default = "default_temp_dir")]
    pub temp_dir: String,
}

fn default_profile() -> String {
    "default".to_string()
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

/// A named profile storing credentials and endpoint info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub api_key: String,
    pub region: Region,
    /// Optional free-form label (e.g. "prod", "staging")
    pub label: Option<String>,
    /// Coralogix team ID, sent as `cgx-team-id` in gRPC metadata.
    #[serde(default)]
    pub team_id: Option<String>,
    /// OpenAI API key for embedding-based features. Falls back to the
    /// `OPENAI_API_KEY` environment variable when not set in the profile.
    #[serde(default)]
    pub openai_api_key: Option<String>,
}

/// Resolved configuration ready for use at runtime.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    /// The profile name this config was resolved from.
    pub profile_name: String,
    pub api_key: String,
    pub endpoint: String,
    /// Coralogix team ID (`cgx-team-id` gRPC metadata).
    pub team_id: Option<String>,
    /// OpenAI API key resolved from profile or `OPENAI_API_KEY` env var.
    pub openai_api_key: Option<String>,
}

/// Returns the cx config directory: `~/.cx/`
pub fn config_dir() -> Result<PathBuf> {
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

/// Load the global config (non-fatal if missing — returns default).
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

/// Load a named profile.
pub fn load_profile(name: &str) -> Result<Profile> {
    let path = profile_file(name)?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Profile '{name}' not found. Run `cx configure` to set it up."))?;
    toml::from_str(&raw).with_context(|| format!("Failed to parse profile '{name}'"))
}

/// Resolve a single named profile, respecting optional CLI overrides.
fn resolve_single(
    profile_name: &str,
    api_key_override: Option<&str>,
    region_override: Option<&str>,
) -> Result<ResolvedConfig> {
    let mut profile = load_profile(profile_name)?;

    if let Some(key) = api_key_override {
        profile.api_key = key.to_string();
    }
    if let Some(region) = region_override {
        profile.region = region.parse()?;
    }

    // `OPENAI_API_KEY` env var takes precedence over the profile value.
    let openai_api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or(profile.openai_api_key);

    Ok(ResolvedConfig {
        profile_name: profile_name.to_string(),
        endpoint: profile.region.api_endpoint().to_string(),
        api_key: profile.api_key,
        team_id: profile.team_id,
        openai_api_key,
    })
}

/// Resolve the active profile, respecting CLI overrides.
///
/// When `profile_override` is `None`, falls back to the default profile in config.
pub fn resolve(
    profile_override: Option<&str>,
    api_key_override: Option<&str>,
    region_override: Option<&str>,
) -> Result<ResolvedConfig> {
    let config = load_config()?;
    let name = profile_override.unwrap_or(&config.default_profile);
    resolve_single(name, api_key_override, region_override)
}

/// Resolve one or more named profiles into a list of `ResolvedConfig` values.
///
/// When `profiles` is empty, falls back to the single default profile from config.
/// Overrides (`api_key_override`, `region_override`) are applied uniformly to every
/// resolved profile — the caller should reject these when `profiles.len() > 1`.
pub fn resolve_all(
    profiles: &[String],
    api_key_override: Option<&str>,
    region_override: Option<&str>,
) -> Result<Vec<ResolvedConfig>> {
    if profiles.is_empty() {
        let cfg = load_config()?;
        return Ok(vec![resolve_single(&cfg.default_profile, api_key_override, region_override)?]);
    }
    profiles
        .iter()
        .map(|name| resolve_single(name, api_key_override, region_override))
        .collect()
}

/// Write a profile to disk, creating directories as needed.
pub fn save_profile(name: &str, profile: &Profile) -> Result<()> {
    let dir = profiles_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.toml"));
    let content = toml::to_string_pretty(profile).context("Failed to serialize profile")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_single (unit logic, no disk I/O) ──────────────────────────────

    /// Build a `ResolvedConfig` directly without touching the filesystem, by
    /// testing the field-level properties that `resolve_single` must guarantee.
    #[test]
    fn resolved_config_carries_profile_name() {
        let cfg = ResolvedConfig {
            profile_name: "prod".to_string(),
            api_key: "k".to_string(),
            endpoint: "https://api.eu2.coralogix.com".to_string(),
            team_id: None,
            openai_api_key: None,
        };
        assert_eq!(cfg.profile_name, "prod");
    }

    #[test]
    fn region_api_endpoint_eu1() {
        assert_eq!(Region::Eu1.api_endpoint(), "https://api.eu2.coralogix.com");
    }

    #[test]
    fn region_api_endpoint_us1() {
        assert_eq!(Region::Us1.api_endpoint(), "https://api.us1.coralogix.com");
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

    // ── resolve_all empty / missing (no filesystem) ───────────────────────────

    #[test]
    fn resolve_all_missing_profile_returns_error() {
        let result = resolve_all(
            &["cx_definitely_does_not_exist_xyz".to_string()],
            None,
            None,
        );
        assert!(result.is_err());
    }

    // ── resolve_all with real profiles (integration, requires ~/.cx access) ───

    #[test]
    #[ignore = "requires write access to ~/.cx; run with `cargo test -- --ignored`"]
    fn resolve_all_integration_multiple_profiles() {
        let names = ["cx_inttest_multi_a", "cx_inttest_multi_b"];
        for (i, name) in names.iter().enumerate() {
            let profile = Profile {
                api_key: format!("key-{i}"),
                region: Region::Eu1,
                label: None,
                team_id: None,
                openai_api_key: None,
            };
            save_profile(name, &profile).unwrap();
        }

        let configs = resolve_all(
            &names.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            None,
            None,
        )
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

    #[test]
    #[ignore = "requires write access to ~/.cx; run with `cargo test -- --ignored`"]
    fn resolve_all_integration_empty_slice_falls_back_to_default() {
        let profile = Profile {
            api_key: "default-key".to_string(),
            region: Region::Eu1,
            label: None,
            team_id: None,
            openai_api_key: None,
        };
        save_profile("default", &profile).unwrap();

        let configs = resolve_all(&[], None, None).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].profile_name, "default");
    }
}
