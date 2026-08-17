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
//! *known* domains in [`CLUSTER_REGIONS`], [`TLD_REGIONS`] and
//! [`BASE_DOMAINS`]. Anything else is [`RegionMatch::Unresolved`], which
//! callers handle by falling back to manual region / endpoint entry (BYOC,
//! private-link, custom domains).

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

/// Known Coralogix **cluster labels** — the segment that sits immediately before
/// the base domain in `<team>.app.<cluster>.coralogix.com` and in the API host
/// `api.<cluster>.coralogix.com` — mapped to their [`Region`]. Only these are
/// recognised; an unknown segment (a typo like `eua2`, or a private cluster)
/// resolves to [`RegionMatch::Unresolved`] rather than being mistaken for the
/// default region.
///
/// Note `cx498` is US2's quirky app-domain cluster (segment != region name).
const CLUSTER_REGIONS: &[(&str, Region)] = &[
    ("us1", Region::Us1),
    ("us2", Region::Us2),
    ("us3", Region::Us3),
    ("eu1", Region::Eu1),
    ("eu2", Region::Eu2),
    ("ap1", Region::Ap1),
    ("ap2", Region::Ap2),
    ("ap3", Region::Ap3),
    ("cx498", Region::Us2),
    ("stg1", Region::Stg1),
];

/// Distinct-TLD app domains where the domain itself determines the region — there
/// is no per-region cluster label (e.g. `<team>.app.coralogix.us` is always US1).
const TLD_REGIONS: &[(&str, Region)] = &[
    ("coralogix.us", Region::Us1),
    ("coralogix.in", Region::Ap1),
    ("coralogixsg.com", Region::Ap2),
];

/// Cluster-bearing base domains and the region used when no cluster label is
/// present (the bare `<team>.coralogix.com` / `<team>.app.coralogix.com` form,
/// which is EU1; `coralogix.net` is staging).
const BASE_DOMAINS: &[(&str, Region)] = &[
    ("coralogix.com", Region::Eu1),
    ("coralogix.net", Region::Stg1),
];

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
    let stripped = host.strip_prefix("api.");
    let is_api_host = stripped.is_some();
    let host = stripped.unwrap_or(&host);

    match region_for_host(host, is_api_host) {
        Some(region) => RegionMatch::Known(region),
        None => RegionMatch::Unresolved,
    }
}

/// Resolve a normalised host (lowercase, no leading `api.`) to a [`Region`].
///
/// `is_api_host` records that a leading `api.` label was stripped: in that
/// form every remaining left-hand label is a *cluster* label, never a team
/// name, so an unknown label must stay unresolved instead of falling back to
/// the base domain's default region.
fn region_for_host(host: &str, is_api_host: bool) -> Option<Region> {
    // 1. Distinct-TLD domains — the TLD alone fixes the region.
    for (suffix, region) in TLD_REGIONS {
        if host_matches_suffix(host, suffix) {
            return Some(region.clone());
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
            return Some(default_region.clone());
        }

        // The cluster label is the last label before the base domain.
        let cluster = prefix.rsplit('.').next().unwrap_or(prefix);

        if let Some((_, region)) = CLUSTER_REGIONS.iter().find(|(c, _)| *c == cluster) {
            return Some(region.clone());
        }

        // No cluster label present — `<team>.app.coralogix.com` or
        // `<team>.coralogix.com` — is the default region (EU1 / stg1). This
        // never applies to API hosts: in `api.<label>.<base>` the label is a
        // cluster, not a team, so `api.mycluster.coralogix.com` (BYOC /
        // private cluster) must stay unresolved rather than become EU1.
        let label_count = prefix.split('.').count();
        if !is_api_host && (cluster == "app" || label_count == 1) {
            return Some(default_region.clone());
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
///   1. If the value is a recognised Coralogix *app* URL (what users paste
///      from the browser), or is exactly a canonical API endpoint, use the
///      derived region.
///   2. Otherwise fall back to [`Region::from_str`]: a known short-name maps to
///      that region; anything else becomes a [`Region::Custom`] endpoint,
///      preserving the existing BYOC override behaviour.
///
/// Endpoint-shaped values that carry information a derived region would lose —
/// an explicit port, a path, a non-https scheme, or an alternate API host such
/// as `ng-api-http.eu2.coralogix.com` — are deliberately **not** coerced to the
/// canonical endpoint: they stay [`Region::Custom`] and are used verbatim,
/// exactly as before URL derivation existed.
pub fn parse_region_arg(s: &str) -> Region {
    if let RegionMatch::Known(region) = region_from_url(s) {
        if is_app_style_input(s) || is_canonical_endpoint(s, &region) {
            return region;
        }
    }
    // Region::from_str is infallible: unknown values become Region::Custom.
    s.parse::<Region>().expect("Region::from_str is infallible")
}

/// True when `input`'s host is app-shaped — a URL a user would paste from the
/// browser rather than an endpoint override: `<team>.app.<cluster>.<base>`,
/// `<team>.app.<base>`, EU1's bare `<team>.<base>`, or a distinct-TLD app
/// domain. API-endpoint-shaped hosts (`api.*`, gateway hosts like
/// `ng-api-http.<cluster>.<base>`) are not app-style.
fn is_app_style_input(input: &str) -> bool {
    let Some(host) = extract_host(input) else {
        return false;
    };
    let host = host.to_ascii_lowercase();

    // The distinct-TLD domains host API/gateway endpoints too
    // (`ng-api-http.coralogix.us` is US1's gateway), so they get the same
    // label-based check as the cluster-bearing base domains — never a blanket
    // "any subdomain is app-style".
    let all_domains = TLD_REGIONS.iter().chain(BASE_DOMAINS.iter());
    for (base, _) in all_domains {
        let Some(prefix) = host.strip_suffix(base) else {
            continue;
        };
        let Some(prefix) = prefix.strip_suffix('.') else {
            continue; // bare base domain or lookalike — not app-style
        };
        let labels: Vec<&str> = prefix.split('.').collect();
        // `<team>.app.<cluster>.<base>` / `<team>.app.<base>` — the `app`
        // label makes this unambiguously a browser URL.
        if labels.contains(&"app") {
            return true;
        }
        // Bare `<team>.<base>` form (EU1 / staging / distinct-TLD): a single
        // label, as long as it doesn't name an API/ingestion service.
        return labels.len() == 1 && !is_service_label(labels[0]);
    }
    false
}

/// True when a lone host label names a Coralogix API/ingestion service rather
/// than a team. Service hosts share domains with team app hosts —
/// `ng-api-http.coralogix.us`, `api.coralogix.us`, `ingress.coralogix.com`,
/// `otel-traces.coralogix.us` — and a `--region` value naming one is a
/// deliberate endpoint override that must stay verbatim, never be coerced to
/// the canonical endpoint of the region its domain implies. A team unluckily
/// matching one of these patterns just misses the coercion convenience and
/// keeps the pre-derivation behaviour.
fn is_service_label(label: &str) -> bool {
    label.contains("api") || label == "ingress" || label.starts_with("otel")
}

/// True when `input` is exactly `region`'s canonical API endpoint — same https
/// scheme, same host, no port, no path, no query — so coercing it to the
/// derived [`Region`] loses nothing.
fn is_canonical_endpoint(input: &str, region: &Region) -> bool {
    let trimmed = input.trim();
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let Ok(url) = url::Url::parse(&candidate) else {
        return false;
    };
    if url.scheme() != "https"
        || url.port().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
    {
        return false;
    }
    url.host_str().is_some_and(|host| {
        format!("https://{}", host.to_ascii_lowercase()) == region.api_endpoint()
    })
}

/// Extract the host from a URL that may be missing its scheme.
///
/// Returns `None` when the input has no host component (empty or unparseable
/// input). Public so interactive prompts can reuse it to validate that a
/// user-supplied endpoint at least carries a host.
pub fn extract_host(input: &str) -> Option<String> {
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

    /// The api-host form of the same rule: in `api.<label>.coralogix.com` the
    /// label is a cluster, so an unknown one (BYOC / private cluster) must not
    /// collapse into the bare-domain EU1 default after `api.` is stripped.
    #[test]
    fn unknown_cluster_api_host_is_unresolved() {
        unresolved("https://api.mycluster.coralogix.com");
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

    /// The canonical API endpoint coerces to its region — losing nothing.
    #[test]
    fn parse_region_arg_canonical_api_endpoint() {
        assert_eq!(
            parse_region_arg("https://api.eu2.coralogix.com").to_string(),
            "eu2"
        );
        assert_eq!(
            parse_region_arg("api.stg1.coralogix.net").to_string(),
            "stg1"
        );
    }

    /// A BYOC-style api host with an unknown cluster must stay a verbatim
    /// custom endpoint (master behaviour), not silently become EU1.
    #[test]
    fn parse_region_arg_unknown_cluster_api_host_stays_custom() {
        let region = parse_region_arg("https://api.mycluster.coralogix.com");
        assert_eq!(region.api_endpoint(), "https://api.mycluster.coralogix.com");
    }

    /// Endpoint-shaped values carrying extra information — an explicit port,
    /// an alternate gateway host — must be used verbatim, not coerced to the
    /// canonical endpoint of the region they mention.
    #[test]
    fn parse_region_arg_endpoint_with_port_stays_custom() {
        let region = parse_region_arg("api.eu2.coralogix.com:8443");
        assert_eq!(region.api_endpoint(), "api.eu2.coralogix.com:8443");
    }

    #[test]
    fn parse_region_arg_gateway_host_stays_custom() {
        let region = parse_region_arg("https://ng-api-http.eu2.coralogix.com");
        assert_eq!(
            region.api_endpoint(),
            "https://ng-api-http.eu2.coralogix.com"
        );
    }

    /// The distinct-TLD domains host gateway/API endpoints too — US1's
    /// gateway is `ng-api-http.coralogix.us`. These are endpoint overrides
    /// and must stay verbatim, exactly like their base-domain equivalents.
    #[test]
    fn parse_region_arg_tld_gateway_host_stays_custom() {
        let region = parse_region_arg("https://ng-api-http.coralogix.us");
        assert_eq!(region.api_endpoint(), "https://ng-api-http.coralogix.us");
    }

    #[test]
    fn parse_region_arg_tld_api_host_stays_custom() {
        let region = parse_region_arg("https://api.coralogix.us");
        assert_eq!(region.api_endpoint(), "https://api.coralogix.us");
    }

    /// EU1's gateway lives on the bare base domain — the same shape as a
    /// bare team app URL (`<team>.coralogix.com`) — and must not be coerced.
    #[test]
    fn parse_region_arg_bare_gateway_host_stays_custom() {
        let region = parse_region_arg("https://ng-api-http.coralogix.com");
        assert_eq!(region.api_endpoint(), "https://ng-api-http.coralogix.com");
    }

    #[test]
    fn parse_region_arg_ingress_host_stays_custom() {
        let region = parse_region_arg("https://ingress.coralogix.com");
        assert_eq!(region.api_endpoint(), "https://ingress.coralogix.com");
    }

    /// App URLs on the distinct-TLD domains still coerce — the `app` label
    /// makes them unambiguous browser URLs.
    #[test]
    fn parse_region_arg_tld_app_url_still_coerces() {
        assert_eq!(
            parse_region_arg("https://myteam.app.coralogix.us").to_string(),
            "us1"
        );
    }

    /// App URLs coerce even with browser noise (path/query) — that's the
    /// paste-your-URL feature, and an app URL is never an endpoint override.
    #[test]
    fn parse_region_arg_app_url_with_path_and_query() {
        assert_eq!(
            parse_region_arg("https://myteam.app.cx498.coralogix.com/logs?id=1").to_string(),
            "us2"
        );
    }
}
