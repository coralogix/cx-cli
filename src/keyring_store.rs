use std::collections::HashMap;

use anyhow::{Context, Result};

const SERVICE_NAME: &str = "cx-cli";
const SECRET_NAMES: &[&str] = &[
    "api_key",
    "oauth_access_token",
    "oauth_refresh_token",
    "oauth_id_token",
    "oauth_token_expiry",
];

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
/// Keeping values separate avoids Windows Credential Manager's 2,560-byte
/// credential-blob limit for OAuth token sets.
pub fn store_secret(profile: &str, key_name: &str, secret: &str) -> Result<()> {
    secret_entry(profile, key_name)?
        .set_password(secret)
        .with_context(|| format!("Failed to store '{key_name}' in keyring"))
}

/// Retrieve a secret from the system keyring.
///
/// Falls back to the legacy combined entry so profiles created by older
/// versions continue to work. Returns `None` if the entry or key does not exist.
pub fn get_secret(profile: &str, key_name: &str) -> Result<Option<String>> {
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
    if let Ok(entry) = secret_entry(profile, key_name) {
        let _ = entry.delete_credential();
    }
    remove_legacy_secret(profile, key_name);
}

/// Delete all secrets for a profile. Best-effort.
pub fn delete_profile(profile: &str) {
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
