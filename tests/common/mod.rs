use std::sync::Arc;

use coralogix_cli::config::ResolvedConfig;
use coralogix_cli::execution::ExecutionTarget;

/// Build an [`ExecutionTarget`] that sends all HTTP traffic to `base_url`.
///
/// Intended for use with a [`wiremock::MockServer`] — pass `mock_server.uri()`
/// as `base_url`.
pub fn test_target(profile_name: &str, base_url: &str) -> Arc<ExecutionTarget> {
    // reqwest with rustls-no-provider needs an explicit crypto provider.
    // `.ok()` ignores the error when another test already installed it.
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let cfg = ResolvedConfig {
        profile_name: profile_name.to_string(),
        api_key: "test-api-key-00000".to_string(),
        endpoint: base_url.to_string(),
    };
    Arc::new(ExecutionTarget::new(cfg).expect("test_target: failed to build ExecutionTarget"))
}
