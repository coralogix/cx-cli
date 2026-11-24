//! Machine-managed credential storage policy.
//!
//! This file is intentionally separate from the user-writable cx configuration.
//! IT can deploy it through Jamf or a Windows management tool to require the
//! platform credential store.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Context, Result};
use serde::Deserialize;

static UNTRUSTED_POLICY_WARNING: OnceLock<()> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PolicyOwner {
    Trusted,
    Untrusted,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Inaccessible,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedPolicy {
    require_credential_storage: String,
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
    match policy.require_credential_storage.as_str() {
        "os_store" => Ok(true),
        value => anyhow::bail!(
            "Invalid require_credential_storage={value:?} in managed policy {}. \
             Expected \"os_store\".",
            path.display()
        ),
    }
}

/// Returns whether the IT policy requires OS credential storage.
///
/// A missing or untrusted policy file means no policy. On Windows, an
/// inaccessible policy path enforces OS storage to prevent a policy bypass. An
/// invalid policy is rejected rather than silently weakening an administrator's
/// intended configuration.
pub fn requires_os_store() -> Result<bool> {
    if !supported_platform() {
        return Ok(false);
    }
    requires_os_store_at(&policy_file()?)
}

fn requires_os_store_at(path: &Path) -> Result<bool> {
    requires_os_store_at_with(path, policy_owner_is_trusted)
}

fn requires_os_store_at_with(
    path: &Path,
    owner_is_trusted: impl Fn(&Path) -> Result<PolicyOwner>,
) -> Result<bool> {
    match policy_directories_are_trusted(path, &owner_is_trusted)? {
        PolicyOwner::Trusted => {}
        PolicyOwner::Untrusted => return untrusted_policy_path(path),
        PolicyOwner::Inaccessible => return Ok(inaccessible_policy_requires_os_store()),
    }
    match std::fs::metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return Ok(inaccessible_policy_requires_os_store());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Cannot access managed policy {}", path.display()));
        }
    }
    match owner_is_trusted(path)? {
        PolicyOwner::Trusted => {}
        PolicyOwner::Untrusted => return untrusted_policy_file(path),
        PolicyOwner::Inaccessible => return Ok(inaccessible_policy_requires_os_store()),
    }
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return Ok(inaccessible_policy_requires_os_store());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to read managed policy {}", path.display()));
        }
    };
    parse_policy(&raw, path)
}

/// On Windows, the policy's `Coralogix` and `cx` directories are part of the
/// trust boundary: their owner could delete an otherwise admin-owned policy
/// file. Only inspect these two product-specific directories; ProgramData and
/// its ancestors are OS-owned locations outside this policy's control.
#[cfg(target_os = "windows")]
fn policy_directories_are_trusted(
    path: &Path,
    owner_is_trusted: &impl Fn(&Path) -> Result<PolicyOwner>,
) -> Result<PolicyOwner> {
    for directory in path.ancestors().skip(1).take(2) {
        match std::fs::metadata(directory) {
            Ok(_) => match owner_is_trusted(directory)? {
                PolicyOwner::Trusted => {}
                trust => return Ok(trust),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Ok(PolicyOwner::Inaccessible);
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Cannot inspect managed policy directory {}",
                        directory.display()
                    )
                });
            }
        }
    }
    Ok(PolicyOwner::Trusted)
}

#[cfg(not(target_os = "windows"))]
fn policy_directories_are_trusted(
    _path: &Path,
    _owner_is_trusted: &impl Fn(&Path) -> Result<PolicyOwner>,
) -> Result<PolicyOwner> {
    Ok(PolicyOwner::Trusted)
}

// On Windows, an inaccessible path under ProgramData could have been created
// by a standard user before IT deploys the policy. Enforcing OS storage avoids
// turning that access denial into a policy bypass. macOS instead treats it as
// absent because enterprise software commonly makes the shared Coralogix
// directory root-only even when no cx policy has been deployed.
#[cfg(target_os = "windows")]
const fn inaccessible_policy_requires_os_store() -> bool {
    true
}

#[cfg(not(target_os = "windows"))]
const fn inaccessible_policy_requires_os_store() -> bool {
    false
}

fn untrusted_policy_warning(path: &Path) -> Result<bool> {
    if UNTRUSTED_POLICY_WARNING.set(()).is_ok() {
        eprintln!(
            "Warning: ignoring managed policy {} because it is not owned by \
             an administrator. Managed credential storage is not enforced.",
            path.display()
        );
    }
    Ok(false)
}

#[cfg(target_os = "windows")]
fn untrusted_policy_file(path: &Path) -> Result<bool> {
    untrusted_policy_path(path)
}

#[cfg(not(target_os = "windows"))]
fn untrusted_policy_file(path: &Path) -> Result<bool> {
    untrusted_policy_warning(path)
}

#[cfg(target_os = "windows")]
fn untrusted_policy_path(path: &Path) -> Result<bool> {
    if UNTRUSTED_POLICY_WARNING.set(()).is_ok() {
        eprintln!(
            "Warning: managed policy path {} is not owned by an administrator. \
             Enforcing OS credential storage until IT secures the policy directories.",
            path.display()
        );
    }
    Ok(true)
}

#[cfg(not(target_os = "windows"))]
fn untrusted_policy_path(path: &Path) -> Result<bool> {
    untrusted_policy_warning(path)
}

#[cfg(target_os = "macos")]
fn policy_owner_is_trusted(path: &Path) -> Result<PolicyOwner> {
    use std::os::unix::fs::MetadataExt;

    Ok(if std::fs::metadata(path)?.uid() == 0 {
        PolicyOwner::Trusted
    } else {
        PolicyOwner::Untrusted
    })
}

#[cfg(target_os = "windows")]
fn policy_owner_is_trusted(path: &Path) -> Result<PolicyOwner> {
    use std::{os::windows::ffi::OsStrExt, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::{LocalFree, ERROR_ACCESS_DENIED},
        Security::{
            Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
            WinBuiltinAdministratorsSid, WinLocalSystemSid, OWNER_SECURITY_INFORMATION, PSID,
        },
    };

    let policy_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut owner: PSID = null_mut();
    let mut descriptor = null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            policy_path.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status == ERROR_ACCESS_DENIED {
        return Ok(PolicyOwner::Inaccessible);
    }
    if status != 0 {
        anyhow::bail!(
            "Cannot determine owner of managed policy {} (Windows error {status})",
            path.display(),
        );
    }
    let trusted = unsafe {
        sid_matches_well_known(owner, WinLocalSystemSid)
            || sid_matches_well_known(owner, WinBuiltinAdministratorsSid)
    };
    unsafe {
        LocalFree(descriptor);
    }
    Ok(if trusted {
        PolicyOwner::Trusted
    } else {
        PolicyOwner::Untrusted
    })
}

#[cfg(target_os = "windows")]
unsafe fn sid_matches_well_known(
    owner: windows_sys::Win32::Security::PSID,
    well_known_sid: windows_sys::Win32::Security::WELL_KNOWN_SID_TYPE,
) -> bool {
    use std::ptr::null_mut;
    use windows_sys::Win32::Security::{CreateWellKnownSid, EqualSid};

    let mut length = 0;
    CreateWellKnownSid(well_known_sid, null_mut(), null_mut(), &mut length);
    let mut sid = vec![0u8; length as usize];
    CreateWellKnownSid(
        well_known_sid,
        null_mut(),
        sid.as_mut_ptr().cast(),
        &mut length,
    ) != 0
        && EqualSid(owner, sid.as_mut_ptr().cast()) != 0
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn policy_owner_is_trusted(_path: &Path) -> Result<PolicyOwner> {
    Ok(PolicyOwner::Untrusted)
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
    fn inaccessible_policy_is_enforced_only_on_windows() {
        assert_eq!(
            inaccessible_policy_requires_os_store(),
            cfg!(target_os = "windows")
        );
    }

    #[test]
    fn policy_file_requires_os_store() {
        let path = write_policy("requires", "require_credential_storage = \"os_store\"");
        let required = requires_os_store_at_with(&path, |_| Ok(PolicyOwner::Trusted));
        let _ = std::fs::remove_file(&path);
        assert!(required.expect("valid policy"));
    }

    #[test]
    fn policy_file_with_invalid_value_fails_closed() {
        let path = write_policy("invalid", "require_credential_storage = \"file\"");
        let result = requires_os_store_at_with(&path, |_| Ok(PolicyOwner::Trusted));
        let _ = std::fs::remove_file(&path);
        let err = result.expect_err("file weakens the policy");
        assert!(
            err.to_string().contains("Expected \"os_store\""),
            "got: {err}"
        );
    }

    #[test]
    fn untrusted_policy_file_is_ignored() {
        let path = write_policy("untrusted", "require_credential_storage = \"os_store\"");
        let required = requires_os_store_at_with(&path, |_| Ok(PolicyOwner::Untrusted));
        let _ = std::fs::remove_file(&path);
        if cfg!(target_os = "windows") {
            assert!(required.expect("Windows enforces an untrusted policy path"));
        } else {
            assert!(!required.expect("untrusted policy is ignored"));
        }
    }

    #[test]
    fn inaccessible_owner_lookup_uses_platform_inaccessible_policy_behavior() {
        let path = write_policy(
            "owner_inaccessible",
            "require_credential_storage = \"os_store\"",
        );
        let required = requires_os_store_at_with(&path, |_| Ok(PolicyOwner::Inaccessible));
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            required.expect("inaccessible owner lookup"),
            cfg!(target_os = "windows")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_rejects_user_owned_policy_directory() {
        let root = std::env::temp_dir().join(format!("cx_policy_parent_{}", std::process::id()));
        let cx_dir = root.join("Coralogix").join("cx");
        std::fs::create_dir_all(&cx_dir).expect("create policy directory");
        let path = cx_dir.join("policy.toml");
        let trust = policy_directories_are_trusted(&path, &|directory| {
            Ok(if directory == cx_dir {
                PolicyOwner::Untrusted
            } else {
                PolicyOwner::Trusted
            })
        })
        .expect("directory ownership check");
        let _ = std::fs::remove_dir_all(root);
        assert_eq!(
            trust,
            PolicyOwner::Untrusted,
            "a user-owned cx directory must not be trusted"
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

    #[test]
    fn policy_rejects_unknown_key() {
        let err = parse_policy(
            "require_credential_store = \"os_store\"",
            Path::new("/managed/policy.toml"),
        )
        .expect_err("unknown policy key must not disable enforcement");
        assert!(format!("{err:#}").contains("unknown field"));
    }

    #[test]
    fn policy_rejects_missing_storage_requirement() {
        let err = parse_policy("", Path::new("/managed/policy.toml"))
            .expect_err("missing storage requirement must not disable enforcement");
        assert!(format!("{err:#}").contains("missing field"));
    }
}
