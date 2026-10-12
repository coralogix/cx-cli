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

/// Apply a removal/insertion set to an in-memory secret map.
///
/// Split out from [`replace_secrets`] so the merge semantics — drop `remove`,
/// write `set`, leave everything else alone — are testable without a keyring.
fn apply_replacement(map: &mut SecretMap, remove: &[&str], set: &[(&str, String)]) {
    for key in remove {
        map.remove(*key);
    }
    for (key, value) in set {
        map.insert((*key).to_string(), value.clone());
    }
}

/// Replace a group of secrets for `profile` in a single checked write.
///
/// Keys named in `remove` are dropped and every pair in `set` is written, in
/// one load-modify-save cycle. Secrets named in neither list are preserved, so
/// this can replace the OAuth token set without disturbing the API keys that
/// share the same keyring entry.
///
/// Unlike [`delete_secret`], failures are surfaced rather than swallowed, and
/// because the whole group goes out in one `save_map` there is no window in
/// which the old secrets are gone and the new ones are not yet stored: either
/// the entry is fully updated or it is left exactly as it was.
pub fn replace_secrets(profile: &str, remove: &[&str], set: &[(&str, String)]) -> Result<()> {
    let mut map = load_map(profile)?.unwrap_or_default();
    apply_replacement(&mut map, remove, set);
    save_map(profile, &map)
}

/// Delete a secret from the system keyring. Best-effort - does not fail if missing.
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

    // ── replace_secrets ───────────────────────────────────────────────────────

    #[test]
    fn apply_replacement_swaps_the_named_group_and_keeps_the_rest() {
        let mut map = SecretMap::new();
        map.insert("api_key".to_string(), "team-key".to_string());
        map.insert("oauth_access_token".to_string(), "old-access".to_string());
        map.insert("oauth_refresh_token".to_string(), "old-refresh".to_string());
        map.insert("oauth_id_token".to_string(), "old-id".to_string());

        apply_replacement(
            &mut map,
            &[
                "oauth_access_token",
                "oauth_refresh_token",
                "oauth_id_token",
            ],
            &[("oauth_access_token", "new-access".to_string())],
        );

        // The new set is written...
        assert_eq!(
            map.get("oauth_access_token").map(String::as_str),
            Some("new-access")
        );
        // ...the stale members of the group are gone, not carried over. This is
        // the whole point: the IdP need not return a refresh or id token, and a
        // leftover one from the previous session must not survive a re-login.
        assert!(!map.contains_key("oauth_refresh_token"));
        assert!(!map.contains_key("oauth_id_token"));
        // ...and secrets outside the group are untouched.
        assert_eq!(map.get("api_key").map(String::as_str), Some("team-key"));
    }

    #[test]
    #[ignore = "requires system keyring access"]
    fn replace_secrets_preserves_unrelated_secrets() {
        let profile = "cx_keyring_test_replace";
        store_secret(profile, "api_key", "team-key").unwrap();
        store_secret(profile, "oauth_access_token", "old-access").unwrap();
        store_secret(profile, "oauth_refresh_token", "old-refresh").unwrap();

        replace_secrets(
            profile,
            &["oauth_access_token", "oauth_refresh_token"],
            &[("oauth_access_token", "new-access".to_string())],
        )
        .unwrap();

        assert_eq!(
            get_secret(profile, "api_key").unwrap(),
            Some("team-key".to_string())
        );
        assert_eq!(
            get_secret(profile, "oauth_access_token").unwrap(),
            Some("new-access".to_string())
        );
        assert_eq!(get_secret(profile, "oauth_refresh_token").unwrap(), None);
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
