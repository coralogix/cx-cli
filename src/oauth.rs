//! OAuth 2.0 + OIDC browser-login flow for Coralogix.
//!
//! Implements:
//!   - OpenID Connect discovery
//!   - PKCE code-challenge / code-verifier generation
//!   - Callback HTTP listener on a randomised port from a fixed allow-list
//!   - Authorization code exchange
//!   - Token refresh
//!   - Token persistence in either the OS keyring or the profile TOML
//!     (selected by `Profile::credential_storage`)

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::seq::SliceRandom;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
use std::time::Duration;

use crate::config::{CredentialStorage, StoredOAuthTokens};
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
pub const KNOWN_ENVIRONMENTS: &[OAuthEnvironment] = &[
    OAuthEnvironment {
        name: "us1",
        client_id: "2b6d0b5c-da8f-477e-907e-50cdad7b90e7",
    },
    OAuthEnvironment {
        name: "us2",
        client_id: "2820e159-5b12-432d-bc09-bef144f9b8c9",
    },
    OAuthEnvironment {
        name: "us3",
        client_id: "a489741c-c875-4652-9b2e-1ba680352b72",
    },
    OAuthEnvironment {
        name: "eu1",
        client_id: "cfb0915d-cb82-436c-b0fb-e8f75e4f9db4",
    },
    OAuthEnvironment {
        name: "eu2",
        client_id: "941d86ab-f652-4430-8153-9af5db5791e5",
    },
    OAuthEnvironment {
        name: "ap1",
        client_id: "091a273c-8194-4ad1-a09e-d787cc5e490a",
    },
    OAuthEnvironment {
        name: "ap2",
        client_id: "d827249c-0699-4abf-8d4c-d1d27dd8fe9a",
    },
    OAuthEnvironment {
        name: "ap3",
        client_id: "7c8d9c61-7d28-4c0f-803b-d4061f33282b",
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

/// Current Unix timestamp in seconds. The system clock is assumed to be after
/// 1970; if it isn't, OAuth refresh wouldn't work anyway.
fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
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
    Some(exp as i64 - unix_now_secs() as i64)
}

// ── OAuth protocol helpers ────────────────────────────────────────────────────

/// Outcome of attempting to bind both loopback stacks on a single candidate port.
enum PortBind {
    /// Both `127.0.0.1` and `[::1]` bound — the ideal case.
    Both(TcpListener, TcpListener),
    /// Only `127.0.0.1` bound because IPv6 loopback is *unavailable* (`::1` is not
    /// a configured address). Safe to use: `localhost` won't reach `::1` either.
    Ipv4Only(TcpListener),
    /// Port cannot be used: IPv4 bind failed, or — critically — `[::1]:port` is
    /// already owned by *another* process. An IPv6-first browser would reach that
    /// other service instead of our callback, so this port must be skipped rather
    /// than served IPv4-only.
    Unusable,
}

/// Try to bind both loopback stacks on `port`.
///
/// Binding the explicit `::1` address is IPv6-only and does not collide with the
/// `127.0.0.1` socket on the same port. An `AddrInUse` error on `::1` means another
/// process holds it (dangerous → `Unusable`); any other `::1` error means IPv6
/// loopback is absent (safe → `Ipv4Only`).
fn try_bind_port(port: u16) -> PortBind {
    let Ok(v4) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)) else {
        return PortBind::Unusable;
    };
    match TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
        Ok(v6) => PortBind::Both(v4, v6),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => PortBind::Unusable,
        Err(_) => PortBind::Ipv4Only(v4),
    }
}

/// Bind the first available port from `CALLBACK_PORTS`, tried in random order.
/// Returns `(listeners, port)` on success.
///
/// Binds **both** loopback stacks on the chosen port: `127.0.0.1` (IPv4, required)
/// and `[::1]` (IPv6, best-effort). The redirect URI handed to the browser uses
/// the hostname `localhost`, which on macOS/Windows resolves to `::1` *first* — so
/// browsers frequently connect over IPv6 before IPv4. Listening only on IPv4 (the
/// previous behaviour) made those callbacks fail with "This site can't be reached".
///
/// Per candidate port (see [`try_bind_port`]):
///   - both stacks bind → use it (preferred).
///   - `::1` absent (IPv6 unavailable) → usable IPv4-only, kept only as a fallback.
///   - `[::1]:port` owned by another process → **skip**, never served IPv4-only,
///     because an IPv6-first browser would otherwise hit that other service and the
///     callback would hang until timeout.
///
/// The IPv4-only fallback is used only when no candidate binds both stacks.
pub fn bind_callback_listener() -> Result<(Vec<TcpListener>, u16)> {
    let mut ports: Vec<u16> = CALLBACK_PORTS.to_vec();
    ports.shuffle(&mut rand::rng());

    // First usable IPv4-only port, kept as a fallback in case no port manages to
    // bind both stacks (e.g. IPv6 loopback disabled on the host).
    let mut ipv4_only_fallback: Option<(Vec<TcpListener>, u16)> = None;

    for &port in &ports {
        match try_bind_port(port) {
            PortBind::Both(v4, v6) => return Ok((vec![v4, v6], port)),
            PortBind::Ipv4Only(v4) if ipv4_only_fallback.is_none() => {
                ipv4_only_fallback = Some((vec![v4], port));
            }
            PortBind::Ipv4Only(_) | PortBind::Unusable => {}
        }
    }

    if let Some(result) = ipv4_only_fallback {
        return Ok(result);
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
/// Polls every listener (IPv4 and IPv6 loopback) so the callback is accepted
/// regardless of which stack the browser connects over. Loops over incoming
/// connections so that browser pre-flight requests (e.g. favicon fetches) do not
/// consume a connection before the real redirect arrives.
///
/// **Note:** Uses synchronous blocking I/O.  Always call via
/// `tokio::task::spawn_blocking` to avoid stalling the async runtime.
fn wait_for_callback_blocking(
    listeners: Vec<TcpListener>,
    expected_state: String,
) -> Result<String> {
    println!("Waiting for browser callback...");
    for listener in &listeners {
        listener
            .set_nonblocking(true)
            .context("Failed to set OAuth callback listener to non-blocking")?;
    }
    loop {
        for listener in &listeners {
            match listener.accept() {
                Ok((stream, _)) => {
                    // Handle the accepted connection with blocking reads/writes.
                    stream
                        .set_nonblocking(false)
                        .context("Failed to set OAuth callback stream to blocking")?;
                    if let Some(code) = extract_and_respond(stream, &expected_state)? {
                        return Ok(code);
                    }
                    // Not an OAuth callback (e.g. favicon request) - keep waiting.
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e).context("OAuth callback listener accept failed"),
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// HTML returned to the browser after a successful OAuth login.
///
const SUCCESS_PAGE_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Authentication successful</title>
<style>
  :root {
    --text-primary: #0e141d;
    --text-secondary: #5e6164;
    --green-badge: #02763a;
  }
  * { box-sizing: border-box; }
  html, body { height: 100%; margin: 0; }
  body {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 100vh;
    padding: 24px;
    background: linear-gradient(160deg, #edeff1 0%, #f7f8f9 45%, #ffffff 100%);
    color: var(--text-primary);
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    -webkit-font-smoothing: antialiased;
    text-rendering: optimizeLegibility;
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: fit-content;
    max-width: calc(100vw - 48px);
    text-align: center;
  }
  .badge {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 42px;
    height: 42px;
    margin-bottom: 8px;
    border-radius: 50%;
    background: var(--green-badge);
  }
  .badge svg { display: block; }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 700;
    line-height: 1.5;
    color: var(--text-primary);
  }
  .subtitle {
    margin: 8px 0 0;
    font-size: 14px;
    font-weight: 500;
    line-height: 1.5;
    color: var(--text-secondary);
    white-space: nowrap;
  }
  @media (max-width: 340px) { .subtitle { white-space: normal; } }
</style>
</head>
<body>
  <main class="card">
    <div class="badge">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" aria-hidden="true" focusable="false">
        <path d="M5 12.5L10 17.5L19.5 7" stroke="#ffffff" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </div>
    <h1>Authentication successful</h1>
    <p class="subtitle">You are now connected to Coralogix.<br>You may close this tab and return to your application.</p>
  </main>
</body>
</html>"##;

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
        // No query string - not an OAuth redirect (e.g. GET / or GET /favicon.ico).
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
            // Query string present but missing code/state - not an OAuth redirect.
            send_http_response(&stream, 204, "");
            return Ok(None);
        }
    };

    if returned_state != expected_state {
        send_http_response(&stream, 400, "State mismatch.");
        bail!("OAuth state mismatch – possible CSRF attempt, aborting.");
    }

    send_http_response(&stream, 200, SUCCESS_PAGE_HTML);
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
    // Must NOT reuse the PKCE challenge - state and code_challenge serve
    // distinct roles and using the same value leaks information.
    let state = {
        let mut rng = rand::rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
        URL_SAFE_NO_PAD.encode(bytes)
    };

    let (listeners, port) = bind_callback_listener()?;
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

    // Always print the URL: when the browser can't be opened (headless run,
    // or cx driven by a coding agent), it is the user's only way in — the
    // agent relays it and the callback still lands on localhost.
    println!("Sign in by visiting:\n  {auth_url}");
    if open::that(&auth_url).is_ok() {
        println!("(opened in your default browser)");
    }

    // Run the blocking TCP listener on a dedicated thread so it does not
    // stall the async runtime.  Bail out if the user takes longer than 5 minutes.
    let code = tokio::time::timeout(
        Duration::from_secs(300),
        tokio::task::spawn_blocking(move || wait_for_callback_blocking(listeners, state)),
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

/// Convert a freshly received `TokenResponse` into the form persisted in the
/// profile TOML when `credential_storage = "file"`.
///
/// `expiry` is computed from `expires_in` when present. When absent, it stays
/// `None` and `resolve_token` falls back to parsing the JWT `exp` claim.
pub fn tokens_to_stored(tokens: &TokenResponse) -> StoredOAuthTokens {
    let expiry = tokens.expires_in.map(|exp_in| unix_now_secs() + exp_in);
    StoredOAuthTokens {
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        id_token: tokens.id_token.clone(),
        expiry,
    }
}

/// Every keyring key [`store_tokens_keyring`] may write.
///
/// Named here so [`replace_tokens_keyring`] can clear the previous token set
/// without touching the API keys that share the same keyring entry.
const OAUTH_KEYRING_KEYS: &[&str] = &[
    "oauth_access_token",
    "oauth_refresh_token",
    "oauth_id_token",
    "oauth_token_expiry",
];

/// The keyring key/value pairs representing `tokens`.
///
/// Only the fields the IdP actually returned are included. In particular the
/// expiry is omitted when the server sends no `expires_in`: `resolve_token`
/// then falls back to parsing the JWT `exp` claim, whereas a sentinel `0`
/// would make every cached token look permanently expired.
fn keyring_entries(tokens: &TokenResponse) -> Vec<(&'static str, String)> {
    let mut entries = vec![("oauth_access_token", tokens.access_token.clone())];
    if let Some(ref rt) = tokens.refresh_token {
        entries.push(("oauth_refresh_token", rt.clone()));
    }
    if let Some(ref it) = tokens.id_token {
        entries.push(("oauth_id_token", it.clone()));
    }
    if let Some(exp_in) = tokens.expires_in {
        entries.push(("oauth_token_expiry", (unix_now_secs() + exp_in).to_string()));
    }
    entries
}

/// Replace the whole OAuth token set for `profile` in the OS keyring.
///
/// Where [`store_tokens_keyring`] merges in only the fields the IdP returned -
/// correct for a silent refresh, which often returns no new refresh token -
/// this drops the previous `oauth_*` set first, so a stale refresh or id token
/// cannot outlive a fresh browser login. Use it only after a full re-login.
///
/// The clear and the write are one checked keyring operation: a failure leaves
/// the previous token set intact rather than deleting a working session and
/// erroring out with nothing to replace it.
pub fn replace_tokens_keyring(profile: &str, tokens: &TokenResponse) -> Result<()> {
    keyring_store::replace_secrets(profile, OAUTH_KEYRING_KEYS, &keyring_entries(tokens))
}

/// Persist an OAuth token set to the OS keyring for `profile`.
///
/// Stores: `oauth_access_token`, `oauth_refresh_token`, `oauth_id_token`,
/// `oauth_token_expiry` (Unix timestamp, computed from `expires_in`).
///
/// Merges rather than replaces: `resolve_token`'s silent refresh calls this and
/// the IdP commonly returns no new refresh token, so any key the response omits
/// must keep its existing value. [`replace_tokens_keyring`] is the counterpart
/// for a full browser re-login, where the old set should not survive.
pub fn store_tokens_keyring(profile: &str, tokens: &TokenResponse) -> Result<()> {
    keyring_store::replace_secrets(profile, &[], &keyring_entries(tokens))
}

fn cached_token_is_valid(token: &str, expiry: Option<u64>) -> bool {
    if let Some(exp) = expiry {
        exp > unix_now_secs() + 30
    } else {
        token_seconds_remaining(token)
            .map(|s| s > 30)
            .unwrap_or(false)
    }
}

/// Resolve a usable bearer token for an OAuth profile.
///
/// Reads the cached access token from the chosen storage backend and returns it
/// if it is still valid (≥ 30 s remaining). Otherwise, uses the cached refresh
/// token to obtain a fresh token set.
///
/// For `OsStore`, the keyring is updated in place and the returned
/// `Option<StoredOAuthTokens>` is always `None`. For `File`, the caller is
/// responsible for persisting the returned `Some(StoredOAuthTokens)` to the
/// profile TOML when a refresh occurred.
///
/// Errors with an actionable message when re-authentication is required.
pub async fn resolve_token(
    profile_name: &str,
    base_url: &str,
    client_id: &str,
    storage: CredentialStorage,
    file_tokens: Option<&StoredOAuthTokens>,
) -> Result<(String, Option<StoredOAuthTokens>)> {
    let (cached_access, cached_expiry, cached_refresh) = match storage {
        CredentialStorage::OsStore => (
            keyring_store::get_secret(profile_name, "oauth_access_token")?,
            keyring_store::get_secret(profile_name, "oauth_token_expiry")?
                .and_then(|s| s.parse::<u64>().ok()),
            keyring_store::get_secret(profile_name, "oauth_refresh_token")?,
        ),
        CredentialStorage::File => {
            let t = file_tokens.ok_or_else(|| {
                anyhow::anyhow!(
                    "OAuth tokens missing for profile '{profile_name}'.\n\
                     Run `cx profiles refresh {profile_name}` to re-authenticate."
                )
            })?;
            (
                Some(t.access_token.clone()),
                t.expiry,
                t.refresh_token.clone(),
            )
        }
    };

    if let Some(ref token) = cached_access {
        if cached_token_is_valid(token, cached_expiry) {
            return Ok((token.clone(), None));
        }
    }

    let refresh_token = cached_refresh.ok_or_else(|| {
        anyhow::anyhow!(
            "OAuth session expired for profile '{profile_name}'.\n\
             Run `cx profiles refresh {profile_name}` to re-authenticate."
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
                 Run `cx profiles refresh {profile_name}` to re-authenticate."
            )
        })?;

    match storage {
        CredentialStorage::OsStore => {
            store_tokens_keyring(profile_name, &tokens)?;
            Ok((tokens.access_token, None))
        }
        CredentialStorage::File => {
            let stored = tokens_to_stored(&tokens);
            Ok((tokens.access_token, Some(stored)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    #[test]
    fn bind_callback_listener_serves_loopback_on_every_bound_stack() {
        let (listeners, port) = bind_callback_listener().expect("a callback port must bind");
        assert!(!listeners.is_empty(), "must bind at least one listener");

        // Every listener shares the single chosen port.
        for l in &listeners {
            assert_eq!(
                l.local_addr().expect("local_addr").port(),
                port,
                "all listeners must share the chosen callback port"
            );
        }

        // IPv4 loopback is mandatory and must accept a connection.
        TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .expect("IPv4 loopback callback must be connectable");

        // When a `::1` listener was bound (IPv6 loopback available), the browser's
        // IPv6-first `localhost` connection must succeed too — this is the fix for
        // the "site can't be reached" callback failure.
        let has_ipv6 = listeners
            .iter()
            .any(|l| l.local_addr().map(|a| a.is_ipv6()).unwrap_or(false));
        if has_ipv6 {
            TcpStream::connect((Ipv6Addr::LOCALHOST, port))
                .expect("IPv6 loopback callback must be connectable when ::1 is bound");
        }
    }

    #[test]
    fn try_bind_port_does_not_fall_back_to_ipv4_when_ipv6_is_occupied() {
        // Squat on an IPv6 loopback port; its IPv4 counterpart stays free.
        let Ok(v6_squatter) = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)) else {
            // No IPv6 loopback on this host — the dangerous scenario can't occur.
            return;
        };
        let port = v6_squatter.local_addr().expect("local_addr").port();

        // `[::1]:port` is owned by another process, so the port must be reported
        // Unusable — NOT served IPv4-only, which an IPv6-first browser would miss.
        match try_bind_port(port) {
            PortBind::Unusable => {}
            PortBind::Both(..) => panic!("::1 was occupied; must not report Both"),
            PortBind::Ipv4Only(_) => {
                panic!("must not fall back to IPv4-only while another process owns ::1")
            }
        }
    }

    fn sample_token_response(expires_in: Option<u64>, refresh: Option<&str>) -> TokenResponse {
        TokenResponse {
            access_token: "access-abc".to_string(),
            token_type: "Bearer".to_string(),
            expires_in,
            refresh_token: refresh.map(str::to_string),
            id_token: Some("id-xyz".to_string()),
            scope: None,
        }
    }

    #[test]
    fn tokens_to_stored_sets_expiry_when_expires_in_present() {
        let before = unix_now_secs();
        let stored = tokens_to_stored(&sample_token_response(Some(3600), Some("refresh-1")));
        let after = unix_now_secs();

        let expiry = stored.expiry.expect("expiry computed from expires_in");
        assert!(expiry >= before + 3600 && expiry <= after + 3600);
        assert_eq!(stored.access_token, "access-abc");
        assert_eq!(stored.refresh_token.as_deref(), Some("refresh-1"));
        assert_eq!(stored.id_token.as_deref(), Some("id-xyz"));
    }

    #[test]
    fn tokens_to_stored_leaves_expiry_none_when_expires_in_absent() {
        let stored = tokens_to_stored(&sample_token_response(None, None));
        assert!(stored.expiry.is_none());
        assert!(stored.refresh_token.is_none());
    }

    #[test]
    fn cached_token_is_valid_uses_expiry_when_present() {
        let now = unix_now_secs();
        // 5 minutes left -> valid.
        assert!(cached_token_is_valid("ignored", Some(now + 300)));
        // Inside the 30 s skew window -> not valid.
        assert!(!cached_token_is_valid("ignored", Some(now + 10)));
        // Already expired.
        assert!(!cached_token_is_valid("ignored", Some(now - 1)));
    }

    #[test]
    fn cached_token_is_valid_falls_back_to_jwt_exp_without_expiry() {
        // Token is not a JWT and there's no separate expiry -> conservatively invalid.
        assert!(!cached_token_is_valid("not-a-jwt", None));
    }

    #[tokio::test]
    async fn resolve_token_file_mode_errors_when_tokens_missing() {
        let err = resolve_token(
            "myprofile",
            "https://example.invalid",
            "client-id",
            CredentialStorage::File,
            None,
        )
        .await
        .expect_err("missing file_tokens must error");
        let msg = format!("{err:#}");
        assert!(msg.contains("OAuth tokens missing"), "got: {msg}");
        assert!(msg.contains("myprofile"), "got: {msg}");
    }

    #[tokio::test]
    async fn resolve_token_file_mode_returns_cached_when_valid() {
        let tokens = StoredOAuthTokens {
            access_token: "still-good".to_string(),
            refresh_token: Some("refresh".to_string()),
            id_token: None,
            expiry: Some(unix_now_secs() + 600),
        };
        // base_url is never contacted because the cached token is valid.
        let (bearer, refreshed) = resolve_token(
            "myprofile",
            "https://example.invalid",
            "client-id",
            CredentialStorage::File,
            Some(&tokens),
        )
        .await
        .expect("cached path must not hit the network");
        assert_eq!(bearer, "still-good");
        assert!(refreshed.is_none());
    }

    #[tokio::test]
    async fn resolve_token_file_mode_errors_when_expired_without_refresh() {
        let tokens = StoredOAuthTokens {
            access_token: "expired".to_string(),
            refresh_token: None,
            id_token: None,
            expiry: Some(unix_now_secs() - 1),
        };
        let err = resolve_token(
            "myprofile",
            "https://example.invalid",
            "client-id",
            CredentialStorage::File,
            Some(&tokens),
        )
        .await
        .expect_err("expired access + no refresh token must error");
        let msg = format!("{err:#}");
        assert!(msg.contains("OAuth session expired"), "got: {msg}");
    }
}
