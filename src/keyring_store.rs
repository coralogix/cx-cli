use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::macos_security::{self, SecurityRunner};

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
/// Each Windows Credential Manager entry is limited to roughly 1,280 UTF-16
/// characters. OAuth persistence falls back to file storage when a value
/// exceeds that limit. On an unsigned or ad-hoc macOS build the write goes
/// through `/usr/bin/security` instead of the Security framework.
pub fn store_secret(profile: &str, key_name: &str, secret: &str) -> Result<()> {
    if security_cli_backend() {
        return store_secret_with(
            &macos_security::SystemSecurityRunner,
            profile,
            key_name,
            secret,
        );
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
    if security_cli_backend() {
        return get_secret_with(&macos_security::SystemSecurityRunner, profile, key_name);
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
    if security_cli_backend() {
        delete_secret_with(&macos_security::SystemSecurityRunner, profile, key_name);
        return;
    }
    if let Ok(entry) = secret_entry(profile, key_name) {
        let _ = entry.delete_credential();
    }
    remove_legacy_secret(profile, key_name);
}

/// Delete all secrets for a profile. Best-effort.
pub fn delete_profile(profile: &str) {
    if security_cli_backend() {
        delete_profile_with(&macos_security::SystemSecurityRunner, profile);
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

/// Unsigned and ad-hoc macOS binaries. Signed macOS and every other OS keep
/// the native keyring backend.
fn security_cli_backend() -> bool {
    macos_security::select_backend(cfg!(target_os = "macos"), apple_developer_id())
        == macos_security::Backend::SecurityCli
}

fn apple_developer_id() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::code_sign::has_apple_developer_id()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

fn service_name(profile: &str) -> String {
    format!("{SERVICE_NAME}:{profile}")
}

fn store_secret_with(
    runner: &impl SecurityRunner,
    profile: &str,
    key_name: &str,
    secret: &str,
) -> Result<()> {
    macos_security::store(runner, &service_name(profile), key_name, secret)
        .with_context(|| format!("Failed to store '{key_name}' in keyring"))
}

fn get_secret_with(
    runner: &impl SecurityRunner,
    profile: &str,
    key_name: &str,
) -> Result<Option<String>> {
    if let Some(secret) = macos_security::get(runner, &service_name(profile), key_name)? {
        return Ok(Some(secret));
    }
    let Some(json) = macos_security::get(runner, SERVICE_NAME, profile)? else {
        return Ok(None);
    };
    let map: SecretMap =
        serde_json::from_str(&json).context("Failed to parse legacy keyring secrets")?;
    Ok(map.get(key_name).cloned())
}

fn delete_secret_with(runner: &impl SecurityRunner, profile: &str, key_name: &str) {
    let _ = macos_security::delete(runner, &service_name(profile), key_name);
    remove_legacy_secret_with(runner, profile, key_name);
}

fn remove_legacy_secret_with(runner: &impl SecurityRunner, profile: &str, key_name: &str) {
    let Ok(Some(json)) = macos_security::get(runner, SERVICE_NAME, profile) else {
        return;
    };
    let Ok(mut map) = serde_json::from_str::<SecretMap>(&json) else {
        return;
    };
    map.remove(key_name);
    if map.is_empty() {
        let _ = macos_security::delete(runner, SERVICE_NAME, profile);
    } else if let Ok(json) = serde_json::to_string(&map) {
        let _ = macos_security::store(runner, SERVICE_NAME, profile, &json);
    }
}

fn delete_profile_with(runner: &impl SecurityRunner, profile: &str) {
    if let Err(err) = macos_security::delete(runner, SERVICE_NAME, profile) {
        eprintln!("Warning: failed to delete legacy keyring entry for {profile}: {err}");
    }
    for key_name in SECRET_NAMES {
        if let Err(err) = macos_security::delete(runner, &service_name(profile), key_name) {
            eprintln!("Warning: failed to delete keyring secret '{key_name}' for {profile}: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use anyhow::Result;

    use super::*;
    use crate::macos_security::{SecurityCommand, SecurityOutput, SecurityRunner};

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

    struct ScriptedRunner {
        calls: Mutex<Vec<SecurityCommand>>,
        responses: Mutex<Vec<SecurityOutput>>,
    }

    impl ScriptedRunner {
        fn new(responses: Vec<SecurityOutput>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                responses: Mutex::new(responses),
            }
        }

        fn calls(&self) -> Vec<SecurityCommand> {
            self.calls.lock().expect("runner lock").clone()
        }
    }

    impl SecurityRunner for ScriptedRunner {
        fn run(&self, command: &SecurityCommand) -> Result<SecurityOutput> {
            self.calls
                .lock()
                .expect("runner lock")
                .push(command.clone());
            let mut responses = self.responses.lock().expect("runner lock");
            if responses.is_empty() {
                anyhow::bail!("no scripted security response left");
            }
            Ok(responses.remove(0))
        }
    }

    fn found(stdout: &str) -> SecurityOutput {
        SecurityOutput {
            status: 0,
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    fn missing() -> SecurityOutput {
        SecurityOutput {
            status: 44,
            stdout: Vec::new(),
            stderr: b"The specified item could not be found in the keychain.\n".to_vec(),
        }
    }

    #[test]
    fn security_get_reads_the_per_secret_item() {
        let runner = ScriptedRunner::new(vec![found("stored-key\n")]);
        let secret = get_secret_with(&runner, "prod", "api_key").unwrap();
        assert_eq!(secret.as_deref(), Some("stored-key"));
        assert!(runner.calls()[0].stdin.contains("\"cx-cli:prod\""));
        assert!(runner.calls()[0].stdin.contains("\"api_key\""));
    }

    #[test]
    fn security_get_falls_back_to_the_legacy_blob() {
        let legacy = r#"{"api_key":"legacy-key"}"#;
        let runner = ScriptedRunner::new(vec![missing(), found(legacy)]);
        let secret = get_secret_with(&runner, "prod", "api_key").unwrap();
        assert_eq!(secret.as_deref(), Some("legacy-key"));
        assert_eq!(runner.calls().len(), 2);
        assert!(
            runner.calls()[1].stdin.contains("find-generic-password"),
            "second lookup should be the legacy combined entry"
        );
        assert!(runner.calls()[1].stdin.contains("\"cx-cli\""));
        assert!(runner.calls()[1].stdin.contains("\"prod\""));
    }

    #[test]
    fn security_get_missing_primary_and_legacy_is_none() {
        let runner = ScriptedRunner::new(vec![missing(), missing()]);
        assert_eq!(get_secret_with(&runner, "prod", "api_key").unwrap(), None);
    }

    #[test]
    fn security_delete_profile_removes_legacy_and_each_known_secret() {
        let responses = vec![missing(); 1 + SECRET_NAMES.len()];
        let runner = ScriptedRunner::new(responses);
        delete_profile_with(&runner, "prod");
        let calls = runner.calls();
        assert_eq!(calls.len(), 1 + SECRET_NAMES.len());
        assert!(calls[0].stdin.contains("delete-generic-password"));
        assert!(calls[0].stdin.contains("\"cx-cli\""));
        assert!(calls[0].stdin.contains("\"prod\""));
        for (call, key_name) in calls.iter().skip(1).zip(SECRET_NAMES) {
            assert!(call.stdin.contains("delete-generic-password"));
            assert!(call.stdin.contains("\"cx-cli:prod\""));
            assert!(
                call.stdin.contains(&format!("\"{key_name}\"")),
                "missing delete for {key_name}: {}",
                call.stdin
            );
        }
    }

    #[test]
    fn security_delete_secret_drops_that_key_from_a_legacy_blob() {
        let legacy = r#"{"api_key":"legacy-key","oauth_access_token":"tok"}"#;
        let runner = ScriptedRunner::new(vec![
            missing(),
            found(legacy),
            SecurityOutput {
                status: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
        ]);
        delete_secret_with(&runner, "prod", "api_key");
        let calls = runner.calls();
        assert_eq!(calls.len(), 3);
        assert!(calls[2].stdin.contains("add-generic-password"));
        assert!(calls[2].stdin.contains("oauth_access_token"));
        assert!(
            !calls[2].stdin.contains("legacy-key"),
            "removed key must not be written back: {}",
            calls[2].stdin
        );
    }
}
