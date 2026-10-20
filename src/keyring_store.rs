use std::collections::HashMap;
#[cfg(debug_assertions)]
use std::path::PathBuf;

#[cfg(all(target_os = "linux", not(target_env = "musl")))]
use std::io::IsTerminal;
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
use std::time::Duration;

use anyhow::{Context, Result};

const SERVICE_NAME: &str = "cx-cli";
const SECRET_NAMES: &[&str] = &[
    "api_key",
    "oauth_access_token",
    "oauth_refresh_token",
    "oauth_id_token",
    "oauth_token_expiry",
];

/// A filesystem-backed keyring used only by debug integration-test binaries.
/// Release builds never include this path; production credentials always use
/// the platform backend.
#[cfg(debug_assertions)]
fn test_secret_path(profile: &str, key_name: &str) -> Option<PathBuf> {
    std::env::var_os("CX_TEST_KEYRING_DIR").map(|root| {
        PathBuf::from(root).join(format!(
            "{}--{}",
            profile.replace(['/', '\\'], "_"),
            key_name
        ))
    })
}
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
const NONINTERACTIVE_LINUX_KEYRING_TIMEOUT: Duration = Duration::from_secs(10);

/// Run a keyring write. On non-interactive glibc Linux, bound the operation so
/// a Secret Service unlock prompt cannot hang a script indefinitely.
///
/// A timed-out worker may finish later, but callers must not update a profile
/// unless this function returns success.
pub async fn write_with_timeout(
    action: impl FnOnce() -> Result<()> + Send + 'static,
) -> Result<()> {
    #[cfg(all(target_os = "linux", not(target_env = "musl")))]
    if !std::io::stdin().is_terminal() {
        return match tokio::time::timeout(
            NONINTERACTIVE_LINUX_KEYRING_TIMEOUT,
            tokio::task::spawn_blocking(action),
        )
        .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => Err(anyhow::anyhow!("OS credential-store worker failed: {err}")),
            Err(_) => Err(anyhow::anyhow!(
                "Timed out waiting for the OS credential store. Unlock your keyring and run \
                 `cx profiles add` from an interactive terminal."
            )),
        };
    }

    action()
}

/// Legacy keyring layout: all secrets for a profile in one JSON blob.
///
/// This is retained only to read and remove credentials written by older
/// versions. Windows Credential Manager limits a credential blob to 2,560
/// bytes, which is not enough for a complete OAuth token set.
type SecretMap = HashMap<String, String>;

fn legacy_entry(profile: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE_NAME, profile).context("Failed to create legacy keyring entry")
}

fn secret_entry(profile: &str, key_name: &str) -> Result<keyring::Entry> {
    let service = format!("{SERVICE_NAME}:{profile}");
    keyring::Entry::new(&service, key_name).context("Failed to create keyring entry")
}

fn load_legacy_map(profile: &str) -> Result<Option<SecretMap>> {
    let entry = legacy_entry(profile)?;
    match entry.get_password() {
        Ok(json) => {
            let map: SecretMap =
                serde_json::from_str(&json).context("Failed to parse legacy keyring secrets")?;
            Ok(Some(map))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("Failed to read legacy keyring: {e}")),
    }
}

fn remove_legacy_secret(profile: &str, key_name: &str) {
    let Ok(Some(mut map)) = load_legacy_map(profile) else {
        return;
    };
    map.remove(key_name);

    let Ok(entry) = legacy_entry(profile) else {
        return;
    };
    if map.is_empty() {
        let _ = entry.delete_credential();
    } else if let Ok(json) = serde_json::to_string(&map) {
        let _ = entry.set_password(&json);
    }
}

/// Store a secret in its own system-keyring entry.
///
/// Each Windows Credential Manager entry is limited to roughly 1,280 UTF-16
/// characters. OAuth persistence falls back to file storage when a value
/// exceeds that limit.
pub fn store_secret(profile: &str, key_name: &str, secret: &str) -> Result<()> {
    #[cfg(debug_assertions)]
    if let Some(path) = test_secret_path(profile, key_name) {
        std::fs::create_dir_all(path.parent().expect("test keyring path has parent"))?;
        return std::fs::write(&path, secret)
            .with_context(|| format!("Failed to store '{key_name}' in test keyring"));
    }

    secret_entry(profile, key_name)?
        .set_password(secret)
        .with_context(|| format!("Failed to store '{key_name}' in keyring"))
}

/// Retrieve a secret from the system keyring.
///
/// Falls back to the legacy combined entry so profiles created by older
/// versions continue to work. Returns `None` if the entry or key does not exist.
pub fn get_secret(profile: &str, key_name: &str) -> Result<Option<String>> {
    #[cfg(debug_assertions)]
    if let Some(path) = test_secret_path(profile, key_name) {
        return match std::fs::read_to_string(path) {
            Ok(secret) => Ok(Some(secret)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).context("Failed to read test keyring secret"),
        };
    }

    let entry = secret_entry(profile, key_name)?;
    match entry.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => {
            Ok(load_legacy_map(profile)?.and_then(|map| map.get(key_name).cloned()))
        }
        Err(e) => Err(anyhow::anyhow!(
            "Failed to read '{key_name}' from keyring: {e}"
        )),
    }
}

/// Delete a secret from the system keyring. Best-effort - does not fail if missing.
pub fn delete_secret(profile: &str, key_name: &str) {
    #[cfg(debug_assertions)]
    if let Some(path) = test_secret_path(profile, key_name) {
        let _ = std::fs::remove_file(path);
        return;
    }

    if let Ok(entry) = secret_entry(profile, key_name) {
        let _ = entry.delete_credential();
    }
    remove_legacy_secret(profile, key_name);
}

/// Delete all secrets for a profile. Best-effort.
pub fn delete_profile(profile: &str) {
    #[cfg(debug_assertions)]
    if test_secret_path(profile, SECRET_NAMES[0]).is_some() {
        for key_name in SECRET_NAMES {
            if let Some(path) = test_secret_path(profile, key_name) {
                let _ = std::fs::remove_file(path);
            }
        }
        return;
    }

    if let Ok(entry) = legacy_entry(profile) {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => {
                eprintln!("Warning: failed to delete legacy keyring entry for {profile}: {e}")
            }
        }
    }
    for key_name in SECRET_NAMES {
        if let Ok(entry) = secret_entry(profile, key_name) {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => eprintln!(
                    "Warning: failed to delete keyring secret '{key_name}' for {profile}: {e}"
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires system keyring access"]
    fn store_and_get_roundtrip() {
        let profile = "cx_keyring_test_roundtrip";
        store_secret(profile, "api_key", "test-secret-123").unwrap();
        let result = get_secret(profile, "api_key").unwrap();
        assert_eq!(result, Some("test-secret-123".to_string()));
        delete_profile(profile);
    }

    #[test]
    #[ignore = "requires system keyring access"]
    fn get_missing_returns_none() {
        let result = get_secret("cx_nonexistent_profile_xyz", "api_key").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    #[ignore = "requires system keyring access"]
    fn delete_removes_secret() {
        let profile = "cx_keyring_test_delete";
        store_secret(profile, "delete_test", "to-be-deleted").unwrap();
        delete_secret(profile, "delete_test");
        let result = get_secret(profile, "delete_test").unwrap();
        assert_eq!(result, None);
        delete_profile(profile);
    }

    #[test]
    #[ignore = "requires system keyring access"]
    fn multiple_secrets_are_stored_separately() {
        let profile = "cx_keyring_test_multikey";
        store_secret(profile, "api_key", "key-1").unwrap();
        store_secret(profile, "oauth_access_token", "token-2").unwrap();
        assert_eq!(
            get_secret(profile, "api_key").unwrap(),
            Some("key-1".to_string())
        );
        assert_eq!(
            get_secret(profile, "oauth_access_token").unwrap(),
            Some("token-2".to_string())
        );
        delete_profile(profile);
    }

    #[test]
    fn known_secret_names_cover_every_profile_secret() {
        assert!(SECRET_NAMES.contains(&"api_key"));
        assert!(SECRET_NAMES.contains(&"oauth_access_token"));
        assert!(SECRET_NAMES.contains(&"oauth_refresh_token"));
        assert!(SECRET_NAMES.contains(&"oauth_id_token"));
        assert!(SECRET_NAMES.contains(&"oauth_token_expiry"));
    }
}
