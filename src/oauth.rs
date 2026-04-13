//! OAuth 2.0 + OIDC browser-login flow for Coralogix.
//!
//! Implements:
//!   - OpenID Connect discovery
//!   - PKCE code-challenge / code-verifier generation
//!   - Callback HTTP listener on a randomised port from a fixed allow-list
//!   - Authorization code exchange
//!   - Token refresh
//!   - Keyring-backed token persistence

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use crate::keyring_store;

// ── Constants ─────────────────────────────────────────────────────────────────

const SCOPES: &[&str] = &["openid", "profile", "email", "offline_access"];

/// Candidate ports for the local OAuth callback listener, tried in random order.
/// The server does not support ephemeral port assignment, so we only use this
/// fixed list.
const CALLBACK_PORTS: &[u16] = &[21783, 24861, 27654, 31847, 38129];

// ── Per-environment metadata ──────────────────────────────────────────────────

/// A known Coralogix deployment with a hard-coded OAuth client ID.
pub struct OAuthEnvironment {
    /// Short region name used in profile TOML (e.g. `"eu2"`).
    pub name: &'static str,
    /// OAuth client ID registered in that environment's IdP.
    pub client_id: &'static str,
}

/// Known Coralogix environments with their OAuth client IDs.
///
/// Client IDs marked `TODO_*` are placeholders to be replaced once per-region
/// IDs are finalised.  The staging environment already has a real client ID.
pub const KNOWN_ENVIRONMENTS: &[OAuthEnvironment] = &[
    OAuthEnvironment {
        name: "us1",
        client_id: "TODO_CLIENT_ID_US1",
    },
    OAuthEnvironment {
        name: "us2",
        client_id: "TODO_CLIENT_ID_US2",
    },
    OAuthEnvironment {
        name: "us3",
        client_id: "TODO_CLIENT_ID_US3",
    },
    OAuthEnvironment {
        name: "eu1",
        client_id: "TODO_CLIENT_ID_EU1",
    },
    OAuthEnvironment {
        name: "eu2",
        client_id: "TODO_CLIENT_ID_EU2",
    },
    OAuthEnvironment {
        name: "ap1",
        client_id: "TODO_CLIENT_ID_AP1",
    },
    OAuthEnvironment {
        name: "ap2",
        client_id: "TODO_CLIENT_ID_AP2",
    },
    OAuthEnvironment {
        name: "ap3",
        client_id: "TODO_CLIENT_ID_AP3",
    },
    OAuthEnvironment {
        name: "stg1",
        client_id: "32cfc1bb-65a1-4c36-9687-6d606a5e0635",
    },
];

// ── Data types ────────────────────────────────────────────────────────────────

/// Token set returned by the IdP after a successful authorisation.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub scope: Option<String>,
}

#[derive(Deserialize)]
struct OpenIdConfig {
    authorization_endpoint: String,
    token_endpoint: String,
}

// ── Utilities ─────────────────────────────────────────────────────────────────

/// Return the hard-coded OAuth client ID for a named region (e.g. `"eu2"`), or `None`.
pub fn client_id_for_region(region_name: &str) -> Option<&'static str> {
    KNOWN_ENVIRONMENTS
        .iter()
        .find(|e| e.name == region_name)
        .map(|e| e.client_id)
}

/// Seconds until the JWT access token expires (`> 0` = still valid, `< 0` = expired).
/// Returns `None` when the JWT structure or `exp` claim is not parseable.
pub fn token_seconds_remaining(access_token: &str) -> Option<i64> {
    let parts: Vec<&str> = access_token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    let exp = json.get("exp")?.as_u64()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    Some(exp as i64 - now as i64)
}

// ── OAuth protocol helpers ────────────────────────────────────────────────────

/// Bind the first available port from `CALLBACK_PORTS`, tried in random order.
/// Returns `(listener, port)` on success.
pub fn bind_callback_listener() -> Result<(TcpListener, u16)> {
    let mut ports: Vec<u16> = CALLBACK_PORTS.to_vec();
    ports.shuffle(&mut rand::rng());
    for &port in &ports {
        if let Ok(listener) = TcpListener::bind(format!("127.0.0.1:{port}")) {
            return Ok((listener, port));
        }
    }
    bail!(
        "Could not bind any OAuth callback port. All ports in the allow-list are in use: {}",
        CALLBACK_PORTS
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn generate_pkce() -> (String, String) {
    let mut rng = rand::rng();
    let verifier_bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
    let verifier = URL_SAFE_NO_PAD.encode(&verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Block the calling thread until the browser delivers the OAuth callback.
/// Validates the `state` parameter and returns the `code`.
///
/// Loops over incoming connections so that browser pre-flight requests
/// (e.g. favicon fetches) do not consume the one accepted connection before
/// the real redirect arrives.
///
/// **Note:** Uses synchronous blocking I/O.  Always call via
/// `tokio::task::spawn_blocking` to avoid stalling the async runtime.
fn wait_for_callback_blocking(listener: TcpListener, expected_state: String) -> Result<String> {
    println!("Waiting for browser callback...");
    loop {
        let (stream, _) = listener.accept()?;
        if let Some(code) = extract_and_respond(stream, &expected_state)? {
            return Ok(code);
        }
        // Not an OAuth callback (e.g. favicon request) — keep waiting.
    }
}

/// Parse one HTTP connection and send an appropriate response.
///
/// Returns `Ok(Some(code))` when a valid OAuth callback is received,
/// `Ok(None)` for unrelated requests, and `Err` on a CSRF state mismatch.
fn extract_and_respond(stream: TcpStream, expected_state: &str) -> Result<Option<String>> {
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    drop(reader); // release the borrow so we can write below

    let path = request_line
        .split_whitespace()
        .nth(1)
        .context("Malformed HTTP request in OAuth callback")?;

    let Some(query) = path.split('?').nth(1) else {
        // No query string — not an OAuth redirect (e.g. GET / or GET /favicon.ico).
        send_http_response(&stream, 204, "");
        return Ok(None);
    };

    let params: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();

    let (code, returned_state) = match (params.get("code"), params.get("state")) {
        (Some(c), Some(s)) => (c.clone(), s.clone()),
        _ => {
            // Query string present but missing code/state — not an OAuth redirect.
            send_http_response(&stream, 204, "");
            return Ok(None);
        }
    };

    if returned_state != expected_state {
        send_http_response(&stream, 400, "State mismatch.");
        bail!("OAuth state mismatch – possible CSRF attempt, aborting.");
    }

    let body = "<html><body><h2>Authentication successful!</h2>\
                <p>You may close this tab and return to the terminal.</p></body></html>";
    send_http_response(&stream, 200, body);
    Ok(Some(code))
}

fn send_http_response(mut stream: &TcpStream, status: u16, body: &str) {
    let status_text = match status {
        200 => "200 OK",
        204 => "204 No Content",
        400 => "400 Bad Request",
        _ => "200 OK",
    };
    let response = if body.is_empty() {
        format!("HTTP/1.1 {status_text}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "HTTP/1.1 {status_text}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    };
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

async fn fetch_openid_config(base_url: &str) -> Result<OpenIdConfig> {
    let url = format!(
        "{}/oauth/.well-known/openid-configuration",
        base_url.trim_end_matches('/')
    );
    reqwest::get(&url)
        .await?
        .error_for_status()
        .with_context(|| format!("OpenID discovery endpoint returned an error: {url}"))?
        .json::<OpenIdConfig>()
        .await
        .with_context(|| format!("Failed to parse OpenID configuration from {url}"))
}

async fn exchange_code(
    token_endpoint: &str,
    client_id: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    let resp = client
        .post(token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Token exchange failed ({status}): {body}");
    }
    resp.json::<TokenResponse>()
        .await
        .context("Failed to parse token exchange response")
}

async fn do_token_refresh(
    token_endpoint: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    let resp = client
        .post(token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Token refresh failed ({status}): {body}");
    }
    resp.json::<TokenResponse>()
        .await
        .context("Failed to parse token refresh response")
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Run the full interactive browser-based OAuth sign-in flow.
///
/// 1. Fetches the OpenID Connect discovery document from `{base_url}/oauth/.well-known/…`
/// 2. Generates a PKCE challenge and a random `state` value
/// 3. Binds a local callback port (randomised from the fixed allow-list)
/// 4. Opens the browser at the authorisation URL
/// 5. Waits for the callback, validates `state`, extracts the code
/// 6. Exchanges the code for tokens and returns them
pub async fn browser_login(base_url: &str, client_id: &str) -> Result<TokenResponse> {
    let oidc = fetch_openid_config(base_url).await?;
    let (verifier, challenge) = generate_pkce();

    // Generate an independent random state for CSRF protection.
    // Must NOT reuse the PKCE challenge — state and code_challenge serve
    // distinct roles and using the same value leaks information.
    let state = {
        let mut rng = rand::rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
        URL_SAFE_NO_PAD.encode(bytes)
    };

    let (listener, port) = bind_callback_listener()?;
    let redirect_uri = format!("http://localhost:{port}/callback");

    let scopes = SCOPES.join(" ");
    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        oidc.authorization_endpoint,
        urlencode(client_id),
        urlencode(&redirect_uri),
        urlencode(&scopes),
        challenge,
        state,
    );

    println!("Opening browser for authentication...");
    if open::that(&auth_url).is_err() {
        println!("Could not open browser automatically.\nPlease visit:\n  {auth_url}");
    }

    // Run the blocking TCP listener on a dedicated thread so it does not
    // stall the async runtime.  Bail out if the user takes longer than 5 minutes.
    let code = tokio::time::timeout(
        Duration::from_secs(300),
        tokio::task::spawn_blocking(move || wait_for_callback_blocking(listener, state)),
    )
    .await
    .context("OAuth login timed out after 5 minutes")?
    .context("OAuth callback task failed")??;

    println!("Authorization code received, exchanging for tokens...");

    exchange_code(
        &oidc.token_endpoint,
        client_id,
        &code,
        &verifier,
        &redirect_uri,
    )
    .await
}

/// Persist an OAuth token set to the OS keyring for `profile`.
///
/// Stores: `oauth_access_token`, `oauth_refresh_token`, `oauth_id_token`,
/// `oauth_token_expiry` (Unix timestamp, computed from `expires_in`).
pub fn store_tokens(profile: &str, tokens: &TokenResponse) -> Result<()> {
    keyring_store::store_secret(profile, "oauth_access_token", &tokens.access_token)?;
    if let Some(ref rt) = tokens.refresh_token {
        keyring_store::store_secret(profile, "oauth_refresh_token", rt)?;
    }
    if let Some(ref it) = tokens.id_token {
        keyring_store::store_secret(profile, "oauth_id_token", it)?;
    }
    // Only store the expiry timestamp when the server provides `expires_in`.
    // When absent, `resolve_token` falls back to parsing the JWT `exp` claim directly.
    // Storing a sentinel `0` would make every cached token appear permanently expired.
    if let Some(exp_in) = tokens.expires_in {
        let expiry = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + exp_in;
        keyring_store::store_secret(profile, "oauth_token_expiry", &expiry.to_string())?;
    }
    Ok(())
}

/// Resolve a usable bearer token for an OAuth profile.
///
/// Returns the cached access token if it is still valid (≥ 30 s remaining).
/// Otherwise, uses the stored refresh token to obtain a fresh token set,
/// persists the new tokens, and returns the new access token.
///
/// Errors with an actionable message when re-authentication is required.
pub async fn resolve_token(profile_name: &str, base_url: &str, client_id: &str) -> Result<String> {
    let access_token = keyring_store::get_secret(profile_name, "oauth_access_token")?;

    let is_valid = if let Some(ref token) = access_token {
        let expiry = keyring_store::get_secret(profile_name, "oauth_token_expiry")?
            .and_then(|s| s.parse::<u64>().ok());
        if let Some(exp) = expiry {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            exp > now + 30
        } else {
            // Fall back to parsing the JWT's `exp` claim.
            token_seconds_remaining(token)
                .map(|s| s > 30)
                .unwrap_or(false)
        }
    } else {
        false
    };

    if is_valid {
        return Ok(access_token.unwrap());
    }

    let refresh_token = keyring_store::get_secret(profile_name, "oauth_refresh_token")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "OAuth session expired for profile '{profile_name}'.\n\
                 Run `cx profiles add {profile_name}` to re-authenticate."
            )
        })?;

    let oidc = fetch_openid_config(base_url).await.with_context(|| {
        format!(
            "Failed to contact OAuth server while refreshing token for profile '{profile_name}'."
        )
    })?;

    let tokens = do_token_refresh(&oidc.token_endpoint, client_id, &refresh_token)
        .await
        .with_context(|| {
            format!(
                "OAuth token refresh failed for profile '{profile_name}'.\n\
                 Run `cx profiles add {profile_name}` to re-authenticate."
            )
        })?;

    store_tokens(profile_name, &tokens)?;
    Ok(tokens.access_token)
}
