//! Machine-managed credential storage policy.
//!
//! This file is intentionally separate from the user-writable cx configuration.
//! IT can deploy it through Jamf or a Windows management tool to require the
//! platform credential store.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ManagedPolicy {
    require_credential_storage: Option<String>,
}

/// Whether this build supports an IT-managed credential policy.
///
/// Linux deliberately remains unchanged, including when `CX_POLICY_FILE` is
/// set, because a consistent OS credential store is not available everywhere.
pub const fn supported_platform() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

fn default_policy_file() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/Coralogix/cx/policy.toml")
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join("Coralogix")
            .join("cx")
            .join("policy.toml")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        PathBuf::new()
    }
}

fn policy_file() -> PathBuf {
    // Test-only escape hatch: production builds always consult the IT-managed
    // location, so a user cannot bypass policy by changing an environment
    // variable.
    #[cfg(test)]
    {
        if let Some(path) = std::env::var_os("CX_POLICY_FILE") {
            return PathBuf::from(path);
        }
    }
    default_policy_file()
}

fn parse_policy(raw: &str, path: &std::path::Path) -> Result<bool> {
    let policy: ManagedPolicy = toml::from_str(raw)
        .with_context(|| format!("Failed to parse managed policy {}", path.display()))?;
    match policy.require_credential_storage.as_deref() {
        None => Ok(false),
        Some("os_store") => Ok(true),
        Some(value) => anyhow::bail!(
            "Invalid require_credential_storage={value:?} in managed policy {}. \
             Expected \"os_store\".",
            path.display()
        ),
    }
}

/// Returns whether the IT policy requires OS credential storage.
///
/// A missing file means no policy. An invalid value is rejected rather than
/// silently weakening an administrator's intended configuration.
pub fn requires_os_store() -> Result<bool> {
    if !supported_platform() {
        return Ok(false);
    }

    let path = policy_file();
    match std::fs::metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Cannot access managed policy {}", path.display()));
        }
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read managed policy {}", path.display()))?;
    parse_policy(&raw, &path)
}

/// Reject storing a credential in a plaintext profile while the IT policy is on.
pub fn ensure_file_storage_allowed() -> Result<()> {
    if requires_os_store()? {
        anyhow::bail!(
            "File credential storage is disabled by your organization's policy. \
             Credentials must be stored in the OS credential store."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_ignores_the_managed_policy() {
        if cfg!(target_os = "linux") {
            assert!(!requires_os_store().expect("Linux ignores policy files"));
        }
    }

    #[test]
    fn default_path_matches_supported_platform() {
        if cfg!(target_os = "macos") {
            assert_eq!(
                default_policy_file(),
                PathBuf::from("/Library/Application Support/Coralogix/cx/policy.toml")
            );
        }
    }

    #[test]
    fn policy_requires_os_store() {
        assert!(parse_policy(
            "require_credential_storage = \"os_store\"",
            std::path::Path::new("/managed/policy.toml")
        )
        .expect("valid policy"));
    }

    #[test]
    fn policy_rejects_unknown_storage() {
        let err = parse_policy(
            "require_credential_storage = \"file\"",
            std::path::Path::new("/managed/policy.toml"),
        )
        .expect_err("file weakens the policy");
        assert!(err.to_string().contains("Expected \"os_store\""));
    }
}
