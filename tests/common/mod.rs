use std::sync::Arc;

use coralogix_cli::config::{AuthKind, ResolvedConfig};
use coralogix_cli::execution::ExecutionTarget;

/// Build an [`ExecutionTarget`] that sends all HTTP traffic to `base_url`.
///
/// Intended for use with a [`wiremock::MockServer`] - pass `mock_server.uri()`
/// as `base_url`.
pub fn test_target(profile_name: &str, base_url: &str) -> Arc<ExecutionTarget> {
    test_target_with_token(profile_name, base_url, "test-api-key-00000")
}

/// Build an [`ExecutionTarget`] with a custom API key/token.
pub fn test_target_with_token(
    profile_name: &str,
    base_url: &str,
    api_key: &str,
) -> Arc<ExecutionTarget> {
    // reqwest with rustls-no-provider needs an explicit crypto provider.
    // `.ok()` ignores the error when another test already installed it.
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let cfg = ResolvedConfig {
        profile_name: profile_name.to_string(),
        api_key: api_key.to_string(),
        auth_kind: AuthKind::ApiKey,
        endpoint: base_url.to_string(),
        default_tier: coralogix_cli::Tier::Archive,
        console_url: None,
        credentials_overridden: false,
    };
    Arc::new(ExecutionTarget::new(cfg).expect("test_target: failed to build ExecutionTarget"))
}

/// `cx` binary command sandboxed to `home`: config is read from `CX_HOME` and
/// credentials go to a file store under it instead of the OS keyring, so tests
/// never touch the developer's real Keychain.
#[allow(dead_code)]
pub fn cx_command(home: &std::path::Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("CX_HOME", home)
        .env("CX_TEST_KEYRING_DIR", home.join("test-keyring"));
    cmd
}
