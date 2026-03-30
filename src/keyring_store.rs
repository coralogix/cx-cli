use anyhow::{Context, Result};

const SERVICE_NAME: &str = "cx-cli";

/// Store a secret in the system keyring.
pub fn store_secret(profile: &str, key_name: &str, secret: &str) -> Result<()> {
    let user = format!("{profile}/{key_name}");
    let entry = keyring::Entry::new(SERVICE_NAME, &user)
        .context("Failed to create keyring entry")?;
    entry
        .set_password(secret)
        .context("Failed to store secret in keyring")?;
    Ok(())
}

/// Retrieve a secret from the system keyring.
/// Returns `None` if the entry does not exist.
pub fn get_secret(profile: &str, key_name: &str) -> Result<Option<String>> {
    let user = format!("{profile}/{key_name}");
    let entry = keyring::Entry::new(SERVICE_NAME, &user)
        .context("Failed to create keyring entry")?;
    match entry.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("Failed to read from keyring: {e}")),
    }
}

/// Delete a secret from the system keyring. Best-effort — does not fail if missing.
pub fn delete_secret(profile: &str, key_name: &str) {
    let user = format!("{profile}/{key_name}");
    if let Ok(entry) = keyring::Entry::new(SERVICE_NAME, &user) {
        let _ = entry.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PROFILE: &str = "cx_keyring_test";

    #[test]
    #[ignore = "requires system keyring access"]
    fn store_and_get_roundtrip() {
        store_secret(TEST_PROFILE, "api_key", "test-secret-123").unwrap();
        let result = get_secret(TEST_PROFILE, "api_key").unwrap();
        assert_eq!(result, Some("test-secret-123".to_string()));
        // cleanup
        delete_secret(TEST_PROFILE, "api_key");
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
        store_secret(TEST_PROFILE, "delete_test", "to-be-deleted").unwrap();
        delete_secret(TEST_PROFILE, "delete_test");
        let result = get_secret(TEST_PROFILE, "delete_test").unwrap();
        assert_eq!(result, None);
    }
}
