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
/// Known Coralogix **cluster labels** — the segment that sits immediately before
/// the base domain in `<team>.app.<cluster>.coralogix.com` and in the API host
/// `api.<cluster>.coralogix.com`. Only these are recognised; an unknown segment
/// (a typo like `eua2`, or a private cluster) resolves to [`RegionMatch::Unresolved`]
/// rather than being mistaken for the default region.
///
/// Note `cx498` is US2's quirky app-domain cluster (segment != region name).
const CLUSTER_REGIONS: &[(&str, &str)] = &[
    ("us1", "us1"),
    ("us2", "us2"),
    ("us3", "us3"),
    ("eu1", "eu1"),
    ("eu2", "eu2"),
    ("ap1", "ap1"),
    ("ap2", "ap2"),
    ("ap3", "ap3"),
    ("cx498", "us2"),
    ("stg1", "stg1"),
];

/// Distinct-TLD app domains where the domain itself determines the region — there
/// is no per-region cluster label (e.g. `<team>.app.coralogix.us` is always US1).
const TLD_REGIONS: &[(&str, &str)] = &[
    ("coralogix.us", "us1"),
    ("coralogix.in", "ap1"),
    ("coralogixsg.com", "ap2"),
];

/// Cluster-bearing base domains and the region used when no cluster label is
/// present (the bare `<team>.coralogix.com` / `<team>.app.coralogix.com` form,
/// which is EU1; `coralogix.net` is staging).
const BASE_DOMAINS: &[(&str, &str)] = &[("coralogix.com", "eu1"), ("coralogix.net", "stg1")];

/// Parse `input` as a Coralogix URL and resolve its [`Region`].
///
/// Accepts URLs with or without a scheme (`https://` is assumed when absent),
/// with any path / query / port. Matching is host-only and case-insensitive; a
/// leading `api.` label is stripped so API hosts resolve identically to app
/// hosts.
///
/// Returns [`RegionMatch::Unresolved`] for hosts that are not a known Coralogix
/// domain — **including coralogix-shaped hosts whose cluster label is unknown**
/// (e.g. `<team>.app.eua2.coralogix.com`) — so the caller falls back to manual
/// entry rather than silently picking the wrong region.
pub fn region_from_url(input: &str) -> RegionMatch {
    let Some(host) = extract_host(input) else {
        return RegionMatch::Unresolved;
    };
    let host = host.to_ascii_lowercase();
    // Strip a leading `api.` so `api.eu2.coralogix.com` reads like `eu2.coralogix.com`.
    let host = host.strip_prefix("api.").unwrap_or(&host);

    match region_for_host(host) {
        Some(region) => RegionMatch::Known(region),
        None => RegionMatch::Unresolved,
    }
}

/// Resolve a normalised host (lowercase, no leading `api.`) to a [`Region`].
fn region_for_host(host: &str) -> Option<Region> {
    // 1. Distinct-TLD domains — the TLD alone fixes the region.
    for (suffix, region) in TLD_REGIONS {
        if host_matches_suffix(host, suffix) {
            return region.parse().ok();
        }
    }

    // 2. Cluster-bearing base domains (coralogix.com / coralogix.net).
    for (base, default_region) in BASE_DOMAINS {
        let Some(prefix) = host.strip_suffix(base) else {
            continue;
        };
        // `strip_suffix` on a non-boundary match (e.g. `evilcoralogix.com`)
        // leaves a prefix not ending in '.'; reject those.
        let prefix = match prefix {
            "" => "", // bare base domain, e.g. `coralogix.com`
            p if p.ends_with('.') => p.trim_end_matches('.'),
            _ => continue, // not a dot-boundary match → lookalike domain
        };

        if prefix.is_empty() {
            // Bare `coralogix.com` with no team/cluster → default region.
            return default_region.parse().ok();
        }

        // The cluster label is the last label before the base domain.
        let cluster = prefix.rsplit('.').next().unwrap_or(prefix);

        if let Some((_, region)) = CLUSTER_REGIONS.iter().find(|(c, _)| *c == cluster) {
            return region.parse().ok();
        }

        // No cluster label present — `<team>.app.coralogix.com` or
        // `<team>.coralogix.com` — is the default region (EU1 / stg1).
        let label_count = prefix.split('.').count();
        if cluster == "app" || label_count == 1 {
            return default_region.parse().ok();
        }

        // A cluster label IS present but is not one we know (typo / private
        // cluster) → unresolved, so we never guess the default region.
        return None;
    }

    None
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

    /// An unknown cluster label (here a typo of `eu2`) must NOT fall through to
    /// the EU1 default — that would silently target the wrong region.
    #[test]
    fn unknown_cluster_label_is_unresolved() {
        unresolved("https://c4c.app.eua2.coralogix.com/olly/chat/abc-123");
    }

    /// `<team>.app.coralogix.com` has no cluster label → EU1.
    #[test]
    fn app_url_eu1_with_app_label() {
        known("https://myteam.app.coralogix.com", "eu1");
    }

    /// A private/unknown cluster on the staging base domain is also unresolved.
    #[test]
    fn unknown_cluster_on_net_is_unresolved() {
        unresolved("https://team.app.wat.coralogix.net");
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
