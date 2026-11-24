//! Platform-native, IT-managed credential storage policy.
//!
//! macOS reads a forced Managed Preferences value and Windows reads a machine
//! registry policy. Neither platform relies on a user-accessible policy file.

use anyhow::Result;

const POLICY_DOMAIN: &str = "com.coralogix.cx";
const POLICY_KEY: &str = "RequireCredentialStorage";
#[cfg(target_os = "windows")]
const WINDOWS_POLICY_PATH: &str = r"SOFTWARE\Policies\Coralogix\cx";

/// Whether this build supports an IT-managed credential policy.
///
/// Linux deliberately remains unchanged because a consistent OS credential
/// store is not available everywhere.
pub const fn supported_platform() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

/// Returns whether IT requires OS credential storage.
///
/// A missing native policy means the device is unmanaged. A present value must
/// be a Boolean on macOS or a DWORD on Windows; unreadable or malformed native
/// policy is an error rather than a silent fallback to file storage.
pub fn requires_os_store() -> Result<bool> {
    #[cfg(target_os = "macos")]
    return requires_os_store_macos();

    #[cfg(target_os = "windows")]
    return requires_os_store_windows();

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Ok(false)
}

fn forced_preference_value(is_forced: bool, value: Option<bool>) -> Result<bool> {
    if !is_forced {
        return Ok(false);
    }
    value.ok_or_else(|| {
        anyhow::anyhow!(
            "Managed preference {POLICY_DOMAIN}/{POLICY_KEY} is forced but has no value"
        )
    })
}

#[cfg(target_os = "macos")]
fn requires_os_store_macos() -> Result<bool> {
    use core_foundation::{
        base::{CFType, TCFType},
        boolean::CFBoolean,
        string::CFString,
    };
    use core_foundation_sys::preferences::{
        CFPreferencesAppValueIsForced, CFPreferencesCopyAppValue,
    };

    let key = CFString::new(POLICY_KEY);
    let domain = CFString::new(POLICY_DOMAIN);
    // A user preference under this domain must not activate the managed policy.
    let is_forced = unsafe {
        CFPreferencesAppValueIsForced(key.as_concrete_TypeRef(), domain.as_concrete_TypeRef())
    };
    let value = if is_forced == 0 {
        None
    } else {
        let value = unsafe {
            CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), domain.as_concrete_TypeRef())
        };
        if value.is_null() {
            None
        } else {
            // SAFETY: CopyAppValue follows the Create Rule and returned a
            // non-null Core Foundation property-list reference.
            let value = unsafe { CFType::wrap_under_create_rule(value) };
            if !value.instance_of::<CFBoolean>() {
                anyhow::bail!("Managed preference {POLICY_DOMAIN}/{POLICY_KEY} must be a Boolean");
            }
            // SAFETY: the type check above guarantees that this is a
            // CFBooleanRef; the get rule retains it so both wrappers can drop
            // independently.
            let value = unsafe { CFBoolean::wrap_under_get_rule(value.as_CFTypeRef().cast()) };
            Some(bool::from(value))
        }
    };
    forced_preference_value(is_forced != 0, value)
}

#[cfg(target_os = "windows")]
fn requires_os_store_windows() -> Result<bool> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr::null_mut};
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD,
    };

    let path: Vec<u16> = OsStr::new(WINDOWS_POLICY_PATH)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let name: Vec<u16> = OsStr::new(POLICY_KEY)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut value = 0u32;
    let mut size = std::mem::size_of_val(&value) as u32;
    // SAFETY: both UTF-16 strings are NUL-terminated and the value buffer is a
    // writable DWORD with its exact byte length in `size`.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&mut value).cast(),
            &mut size,
        )
    };
    registry_policy_value(status, size, value)
}

#[cfg(target_os = "windows")]
fn registry_policy_value(status: u32, size: u32, value: u32) -> Result<bool> {
    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
    };

    match status {
        ERROR_SUCCESS if size == std::mem::size_of_val(&value) as u32 => Ok(value != 0),
        ERROR_SUCCESS => anyhow::bail!(
            "Registry policy HKLM\\{WINDOWS_POLICY_PATH}\\{POLICY_KEY} has an invalid DWORD size {size}"
        ),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        status => anyhow::bail!(
            "Cannot read registry policy HKLM\\{WINDOWS_POLICY_PATH}\\{POLICY_KEY} (Windows error {status})"
        ),
    }
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
    fn supported_platform_matches_native_policy_readers() {
        assert_eq!(
            supported_platform(),
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
    }

    #[test]
    fn policy_identity_is_stable() {
        assert_eq!(POLICY_DOMAIN, "com.coralogix.cx");
        assert_eq!(POLICY_KEY, "RequireCredentialStorage");
    }

    #[test]
    fn unmanaged_preference_does_not_enforce_storage() {
        assert!(
            !forced_preference_value(false, Some(true)).expect("unmanaged preference is ignored")
        );
    }

    #[test]
    fn forced_boolean_preference_enforces_its_value() {
        assert!(forced_preference_value(true, Some(true)).expect("forced true"));
        assert!(!forced_preference_value(true, Some(false)).expect("forced false"));
    }

    #[test]
    fn forced_preference_without_value_is_rejected() {
        let err =
            forced_preference_value(true, None).expect_err("a forced preference needs a value");
        assert!(err.to_string().contains("has no value"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_reader_uses_the_managed_preference_domain() {
        // The real reader cannot be exercised without an MDM profile; this
        // verifies the domain and key used for that profile.
        assert_eq!(POLICY_DOMAIN, "com.coralogix.cx");
        assert_eq!(POLICY_KEY, "RequireCredentialStorage");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_reader_uses_the_machine_policy_location() {
        use windows_sys::Win32::Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
        };

        let size = std::mem::size_of::<u32>() as u32;
        assert!(registry_policy_value(ERROR_SUCCESS, size, 1).expect("enabled DWORD"));
        assert!(!registry_policy_value(ERROR_SUCCESS, size, 0).expect("disabled DWORD"));
        assert!(!registry_policy_value(ERROR_FILE_NOT_FOUND, size, 0).expect("missing value"));
        assert!(!registry_policy_value(ERROR_PATH_NOT_FOUND, size, 0).expect("missing key"));
        assert!(registry_policy_value(ERROR_SUCCESS, 1, 1).is_err());
        assert_eq!(WINDOWS_POLICY_PATH, r"SOFTWARE\Policies\Coralogix\cx");
        assert_eq!(POLICY_KEY, "RequireCredentialStorage");
    }
}
