//! Region derivation from Coralogix URLs.
//!
//! Users frequently don't know their Coralogix *region* short-name (`eu2`,
//! `us1`, …) but they always have a browser tab open on their team. This module
//! turns a pasted Coralogix URL into a known [`Region`], so onboarding can ask
//! "paste your URL" instead of "which region are you in?".
//!
//! ## Why a table and not a heuristic
//!
//! The subdomain segment in the *app* URL is **not** the segment in the *API*
//! host. US2's app domain is `<team>.app.cx498.coralogix.com`, but its API host
//! is `api.us2.coralogix.com` — `cx498 != us2`. So we cannot construct an API
//! endpoint by string-munging an arbitrary URL; we can only recognise the
//! *known* domains in [`REGION_DOMAINS`]. Anything else is [`RegionMatch::Unresolved`],
//! which callers handle by falling back to manual region / endpoint entry
//! (BYOC, private-link, custom domains).

use crate::config::Region;

/// Outcome of deriving a region from a pasted URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionMatch {
    /// The URL's host matched a known Coralogix domain.
    Known(Region),
    /// The URL is not a recognised Coralogix domain (BYOC, private-link, custom
    /// deployment, or not a URL at all). Callers fall back to manual entry —
    /// we never fabricate an API endpoint from an unknown host.
    Unresolved,
}

/// Known Coralogix app/API domain suffixes mapped to their region short-name.
///
/// Ordered **most-specific first**: the matcher walks this list in order and
/// takes the first host-suffix hit, so region-specific domains (e.g.
/// `eu2.coralogix.com`, `cx498.coralogix.com`) are tested before the bare
/// `coralogix.com` catch-all that identifies EU1.
///
/// Suffixes are matched against the URL host after a leading `api.` is stripped,
/// so both the app hostname (`<team>.app.<suffix>`) and the API hostname
/// (`api.<region>.coralogix.com`) resolve to the same region.
const REGION_DOMAINS: &[(&str, &str)] = &[
    // Region-specific domains (must precede the bare coralogix.com fallback).
    ("us1.coralogix.com", "us1"),
    ("cx498.coralogix.com", "us2"), // US2 app domain (quirky: segment != region)
    ("us2.coralogix.com", "us2"),   // US2 API host
    ("us3.coralogix.com", "us3"),
    ("eu2.coralogix.com", "eu2"),
    ("ap3.coralogix.com", "ap3"),
    ("ap1.coralogix.com", "ap1"), // AP1 API host
    ("ap2.coralogix.com", "ap2"), // AP2 API host
    ("eu1.coralogix.com", "eu1"), // EU1 API host
    // Distinct-TLD app domains.
    ("coralogix.us", "us1"),
    ("coralogix.in", "ap1"),
    ("coralogixsg.com", "ap2"),
    // Staging.
    ("coralogix.net", "stg1"),
    // Bare catch-all: `<team>.coralogix.com` is EU1's app domain. Kept last so
    // it only matches when no region-specific suffix above did.
    ("coralogix.com", "eu1"),
];

/// Parse `input` as a Coralogix URL and resolve its [`Region`].
///
/// Accepts URLs with or without a scheme (`https://` is assumed when absent),
/// with any path / query / port. Matching is host-only, case-insensitive, and a
/// leading `api.` label is stripped so API hosts resolve identically to app
/// hosts.
///
/// Returns [`RegionMatch::Unresolved`] for hosts that are not a known Coralogix
/// domain — including coralogix-shaped-but-unknown clusters — so the caller can
/// fall back to manual entry rather than guess a wrong endpoint.
pub fn region_from_url(input: &str) -> RegionMatch {
    let Some(host) = extract_host(input) else {
        return RegionMatch::Unresolved;
    };
    let host = host.to_ascii_lowercase();
    // Strip a leading `api.` so `api.eu2.coralogix.com` matches `eu2.coralogix.com`.
    let host = host.strip_prefix("api.").unwrap_or(&host);

    for (suffix, region_name) in REGION_DOMAINS {
        if host_matches_suffix(host, suffix) {
            // Every table entry names a valid region short-name, so this parse
            // is infallible for known regions.
            if let Ok(region) = region_name.parse::<Region>() {
                return RegionMatch::Known(region);
            }
        }
    }
    RegionMatch::Unresolved
}

/// Resolve a `--region` argument value that may be a short-name, a full custom
/// endpoint URL, or a Coralogix app/API URL.
///
/// Resolution order:
///   1. If the value is a recognised Coralogix URL, use the derived region.
///   2. Otherwise fall back to [`Region::from_str`]: a known short-name maps to
///      that region; anything else becomes a [`Region::Custom`] endpoint,
///      preserving the existing BYOC override behaviour.
pub fn parse_region_arg(s: &str) -> Region {
    if let RegionMatch::Known(region) = region_from_url(s) {
        return region;
    }
    // Infallible: Region::from_str maps unknown values to Region::Custom.
    s.parse::<Region>()
        .unwrap_or_else(|_| Region::Custom(s.to_string()))
}

/// Extract the host from a URL that may be missing its scheme.
///
/// Returns `None` when the input has no host component (e.g. a bare region
/// short-name like `eu2`, which the `url` crate parses without a host).
fn extract_host(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    // `url::Url` requires a scheme. Prepend one when absent so schemeless input
    // like `myteam.app.eu2.coralogix.com/logs` still parses to a host.
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&candidate).ok()?;
    parsed.host_str().map(str::to_string)
}

/// True when `host` equals `suffix` or ends with `.suffix` (a subdomain of it).
///
/// The dot-boundary check prevents `evilcoralogix.com` from matching the
/// `coralogix.com` suffix.
fn host_matches_suffix(host: &str, suffix: &str) -> bool {
    host == suffix
        || host
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(input: &str, expected: &str) {
        match region_from_url(input) {
            RegionMatch::Known(r) => assert_eq!(
                r.to_string(),
                expected,
                "input {input:?} should resolve to {expected}"
            ),
            RegionMatch::Unresolved => panic!("input {input:?} should resolve to {expected}"),
        }
    }

    fn unresolved(input: &str) {
        assert_eq!(
            region_from_url(input),
            RegionMatch::Unresolved,
            "input {input:?} should be Unresolved"
        );
    }

    // ── App URLs (what users paste from the browser) ──────────────────────────

    #[test]
    fn app_url_eu2() {
        known("https://myteam.app.eu2.coralogix.com", "eu2");
    }

    /// The crux case: US2's app domain segment (`cx498`) is NOT its region name.
    #[test]
    fn app_url_us2_cx498() {
        known("https://myteam.app.cx498.coralogix.com", "us2");
    }

    #[test]
    fn app_url_us3() {
        known("https://myteam.app.us3.coralogix.com", "us3");
    }

    #[test]
    fn app_url_ap3() {
        known("https://myteam.app.ap3.coralogix.com", "ap3");
    }

    /// EU1's app domain is the bare `<team>.coralogix.com` — no `app.` label,
    /// no region segment. Must resolve via the catch-all, not shadow others.
    #[test]
    fn app_url_eu1_bare() {
        known("https://myteam.coralogix.com", "eu1");
    }

    // ── Distinct-TLD app domains ──────────────────────────────────────────────

    #[test]
    fn app_url_us1_dot_us() {
        known("https://myteam.app.coralogix.us", "us1");
    }

    #[test]
    fn app_url_ap1_dot_in() {
        known("https://myteam.app.coralogix.in", "ap1");
    }

    #[test]
    fn app_url_ap2_sg() {
        known("https://myteam.app.coralogixsg.com", "ap2");
    }

    // ── API hosts (leading api. stripped) ─────────────────────────────────────

    #[test]
    fn api_host_eu2() {
        known("https://api.eu2.coralogix.com", "eu2");
    }

    #[test]
    fn api_host_us1() {
        known("https://api.us1.coralogix.com", "us1");
    }

    // ── Staging ───────────────────────────────────────────────────────────────

    #[test]
    fn staging_net() {
        known("https://myteam.app.stg1.coralogix.net", "stg1");
    }

    // ── Input tolerance ───────────────────────────────────────────────────────

    #[test]
    fn schemeless_input() {
        known("myteam.app.eu2.coralogix.com", "eu2");
    }

    #[test]
    fn trailing_path_and_query() {
        known("https://myteam.app.eu2.coralogix.com/logs?foo=bar", "eu2");
    }

    #[test]
    fn trailing_slash() {
        known("https://myteam.app.eu2.coralogix.com/", "eu2");
    }

    #[test]
    fn uppercase_host() {
        known("https://MyTeam.App.EU2.Coralogix.Com", "eu2");
    }

    // ── Unresolved (BYOC / private-link / custom / garbage) ───────────────────

    #[test]
    fn byoc_custom_domain_unresolved() {
        unresolved("https://coralogix.mycompany.internal");
    }

    #[test]
    fn lookalike_domain_not_matched() {
        // Must not match the `coralogix.com` suffix without a dot boundary.
        unresolved("https://evilcoralogix.com");
    }

    #[test]
    fn empty_input_unresolved() {
        unresolved("");
    }

    #[test]
    fn bare_region_shortname_is_unresolved_as_url() {
        // Not a URL host we recognise; parse_region_arg handles short-names.
        unresolved("eu2");
    }

    // ── parse_region_arg ──────────────────────────────────────────────────────

    #[test]
    fn parse_region_arg_shortname() {
        assert_eq!(parse_region_arg("eu2").to_string(), "eu2");
    }

    #[test]
    fn parse_region_arg_coralogix_url() {
        assert_eq!(
            parse_region_arg("https://myteam.app.cx498.coralogix.com").to_string(),
            "us2"
        );
    }

    #[test]
    fn parse_region_arg_custom_url_stays_custom() {
        let region = parse_region_arg("https://api.mycompany.internal");
        assert_eq!(region.api_endpoint(), "https://api.mycompany.internal");
    }

    #[test]
    fn parse_region_arg_schemeless_coralogix_url() {
        assert_eq!(
            parse_region_arg("myteam.app.eu2.coralogix.com").to_string(),
            "eu2"
        );
    }
}
