//! Machine-managed credential storage policy.
//!
//! This file is intentionally separate from the user-writable cx configuration.
//! IT can deploy it through Jamf or a Windows management tool to require the
//! platform credential store.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ManagedPolicy {
    require_credential_storage: Option<String>,
}

/// Whether this build supports an IT-managed credential policy.
///
/// Linux deliberately remains unchanged because a consistent OS credential
/// store is not available everywhere.
pub const fn supported_platform() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

fn default_policy_file() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Ok(PathBuf::from(
            "/Library/Application Support/Coralogix/cx/policy.toml",
        ))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(windows_program_data()?
            .join("Coralogix")
            .join("cx")
            .join("policy.toml"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(PathBuf::new())
    }
}

/// `%SystemDrive%\ProgramData`, with the drive taken from the system Windows
/// directory. The `ProgramData` and `SystemDrive` environment variables are
/// not consulted because a user can set them for their own processes.
#[cfg(target_os = "windows")]
fn windows_program_data() -> Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;

    let mut buffer = [0u16; 260];
    // SAFETY: the pointer and length describe `buffer`, which outlives the call.
    let len =
        unsafe { GetSystemWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if len == 0 || len >= buffer.len() {
        anyhow::bail!("Could not resolve the Windows directory to locate the managed policy");
    }
    let windows_dir = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len]));
    let system_drive = windows_dir
        .ancestors()
        .last()
        .unwrap_or(windows_dir.as_path());
    Ok(system_drive.join("ProgramData"))
}

/// Every build reads the machine-wide policy file. An environment variable
/// must not be able to redirect or disable an IT-managed policy.
fn policy_file() -> Result<PathBuf> {
    default_policy_file()
}

fn parse_policy(raw: &str, path: &Path) -> Result<bool> {
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
    requires_os_store_at(&policy_file()?)
}

fn requires_os_store_at(path: &Path) -> Result<bool> {
    match std::fs::metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Cannot access managed policy {}", path.display()));
        }
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read managed policy {}", path.display()))?;
    parse_policy(&raw, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_policy(test: &str, body: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("cx_policy_{test}_{}.toml", std::process::id()));
        std::fs::write(&path, body).expect("write policy");
        path
    }

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
                default_policy_file().expect("macOS policy path"),
                PathBuf::from("/Library/Application Support/Coralogix/cx/policy.toml")
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_path_ignores_the_program_data_environment_variable() {
        let expected = default_policy_file().expect("Windows policy path");
        assert!(
            expected.ends_with(r"ProgramData\Coralogix\cx\policy.toml"),
            "got {}",
            expected.display()
        );

        let original = std::env::var_os("ProgramData");
        std::env::set_var("ProgramData", r"C:\nowhere");
        let redirected = default_policy_file();
        match original {
            Some(value) => std::env::set_var("ProgramData", value),
            None => std::env::remove_var("ProgramData"),
        }
        assert_eq!(redirected.expect("Windows policy path"), expected);
    }

    #[test]
    fn missing_policy_file_means_no_policy() {
        let path = std::env::temp_dir().join("cx_policy_that_does_not_exist.toml");
        assert!(!requires_os_store_at(&path).expect("missing file is no policy"));
    }

    #[test]
    fn policy_file_requires_os_store() {
        let path = write_policy("requires", "require_credential_storage = \"os_store\"");
        let required = requires_os_store_at(&path);
        let _ = std::fs::remove_file(&path);
        assert!(required.expect("valid policy"));
    }

    #[test]
    fn policy_file_with_invalid_value_fails_closed() {
        let path = write_policy("invalid", "require_credential_storage = \"file\"");
        let result = requires_os_store_at(&path);
        let _ = std::fs::remove_file(&path);
        let err = result.expect_err("file weakens the policy");
        assert!(
            err.to_string().contains("Expected \"os_store\""),
            "got: {err}"
        );
    }

    #[test]
    fn policy_requires_os_store() {
        assert!(parse_policy(
            "require_credential_storage = \"os_store\"",
            Path::new("/managed/policy.toml")
        )
        .expect("valid policy"));
    }

    #[test]
    fn policy_rejects_unknown_storage() {
        let err = parse_policy(
            "require_credential_storage = \"file\"",
            Path::new("/managed/policy.toml"),
        )
        .expect_err("file weakens the policy");
        assert!(err.to_string().contains("Expected \"os_store\""));
    }
}
