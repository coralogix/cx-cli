use std::collections::HashMap;

use anyhow::{Context, Result};

const SERVICE_NAME: &str = "cx-cli";

/// All secrets for a profile, stored as a single JSON blob in one keyring entry.
type SecretMap = HashMap<String, String>;

fn load_map(profile: &str) -> Result<Option<SecretMap>> {
    let entry =
        keyring::Entry::new(SERVICE_NAME, profile).context("Failed to create keyring entry")?;
    match entry.get_password() {
        Ok(json) => {
            let map: SecretMap =
                serde_json::from_str(&json).context("Failed to parse keyring secrets")?;
            Ok(Some(map))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("Failed to read from keyring: {e}")),
    }
}

fn save_map(profile: &str, map: &SecretMap) -> Result<()> {
    let entry =
        keyring::Entry::new(SERVICE_NAME, profile).context("Failed to create keyring entry")?;
    if map.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => eprintln!("Warning: failed to delete keyring entry for {profile}: {e}"),
        }
    } else {
        let json = serde_json::to_string(map).context("Failed to serialize keyring secrets")?;
        entry
            .set_password(&json)
            .context("Failed to store secrets in keyring")?;
    }
    Ok(())
}

/// Store a secret in the system keyring.
pub fn store_secret(profile: &str, key_name: &str, secret: &str) -> Result<()> {
    let mut map = load_map(profile)?.unwrap_or_default();
    map.insert(key_name.to_string(), secret.to_string());
    save_map(profile, &map)
}

/// Retrieve a secret from the system keyring.
/// Returns `None` if the entry or key does not exist.
pub fn get_secret(profile: &str, key_name: &str) -> Result<Option<String>> {
    Ok(load_map(profile)?.and_then(|m| m.get(key_name).cloned()))
}

/// Delete a secret from the system keyring. Best-effort — does not fail if missing.
pub fn delete_secret(profile: &str, key_name: &str) {
    let Ok(Some(mut map)) = load_map(profile) else {
        return;
    };
    map.remove(key_name);
    let _ = save_map(profile, &map);
}

/// Delete all secrets for a profile. Best-effort.
pub fn delete_profile(profile: &str) {
    if let Ok(entry) = keyring::Entry::new(SERVICE_NAME, profile) {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => eprintln!("Warning: failed to delete keyring entry for {profile}: {e}"),
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
    fn multiple_keys_single_entry() {
        let profile = "cx_keyring_test_multikey";
        store_secret(profile, "api_key", "key-1").unwrap();
        store_secret(profile, "openai_api_key", "key-2").unwrap();
        assert_eq!(
            get_secret(profile, "api_key").unwrap(),
            Some("key-1".to_string())
        );
        assert_eq!(
            get_secret(profile, "openai_api_key").unwrap(),
            Some("key-2".to_string())
        );
        delete_profile(profile);
    }
}
