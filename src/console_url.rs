//! Pure builders for "View in Coralogix" web console links.
//!
//! These functions take an already-resolved console base URL (scheme +
//! team subdomain + console domain, e.g. `https://acme.app.eu2.coralogix.com`)
//! and an entity ID, and return the full console URL for that entity.
//!
//! No I/O happens here - resolving the base URL (via `/identity/whoami` and
//! region metadata) is handled by `ExecutionTarget::console_base` in
//! `crate::execution`. Keeping these as pure string builders makes them
//! trivial to unit test.
//!
//! ## Hash routing
//!
//! The Coralogix web console is a single-page app that routes client-side
//! off a `#/` hash fragment - the server only ever serves the app shell, so
//! a link without the `#/` prefix silently loads the app's default screen
//! instead of the intended entity. This is documented (with concrete
//! examples) for dashboards and Explore deep links:
//! - Dashboards: `https://<team>.<domain>/#/dashboards/<dashboard_id>` (see
//!   "Share Dashboard URLs",
//!   <https://coralogix.com/docs/user-guides/custom-dashboards/tutorials/share-dashboard-content/>)
//! - Explore (incl. saved views): `https://<team>.<domain>/#/explore?<params>`,
//!   e.g. `?viewId=<saved_view_id>` (see "Deep links and URL parameters",
//!   <https://coralogix.com/docs/user-guides/data_exploration/deep-links/>)
//! - Alerts: `https://<team>.<domain>/#/alerts/<alert_id>` (documented
//!   pattern for alert deep links referenced from runbooks/webhooks)
//!
//! All builders below include the `#/` prefix accordingly. Query parameters
//! on a hash-routed page live *inside* the fragment (`#/path?query=...`),
//! not after a bare path, matching the Explore examples above.

use url::form_urlencoded;

/// Strip a single trailing `/` from `base`, if present, so callers can join
/// `{base}/#/{path}` without producing a double slash before the hash.
fn trim_base(base: &str) -> &str {
    base.trim_end_matches('/')
}

/// Build the console URL for a dashboard: `{base}/#/dashboards/{id}`.
pub fn dashboard_url(base: &str, id: &str) -> String {
    format!("{}/#/dashboards/{id}", trim_base(base))
}

/// Build the console URL for an alert: `{base}/#/alerts/{id}`.
pub fn alert_url(base: &str, id: &str) -> String {
    format!("{}/#/alerts/{id}", trim_base(base))
}

/// Build the console URL for a case: `{base}/#/cases?id={urlencoded id}`.
///
/// The `#/cases` route and the case ID's role in "sharing a case with a
/// teammate" are documented, but the exact query parameter name is not
/// published anywhere we could find - `id` is our best-effort guess,
/// consistent with the shape of other Coralogix deep links. See the PR
/// description's "Command coverage" section for the caveat.
pub fn case_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!("{}/#/cases?id={encoded}", trim_base(base))
}

/// Build the console URL for a saved Explore view: `{base}/#/explore?viewId={id}`.
///
/// `cx views` manages the same "saved view" entity referenced by Explore's
/// documented `viewId` deep-link parameter (both are backed by the
/// `data-exploration/views` API namespace) - see "Deep links and URL
/// parameters", <https://coralogix.com/docs/user-guides/data_exploration/deep-links/>.
pub fn view_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!("{}/#/explore?viewId={encoded}", trim_base(base))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_url_joins_base_and_id() {
        assert_eq!(
            dashboard_url("https://acme.app.eu2.coralogix.com", "dash-abc123"),
            "https://acme.app.eu2.coralogix.com/#/dashboards/dash-abc123"
        );
    }

    #[test]
    fn dashboard_url_trims_trailing_slash_on_base() {
        assert_eq!(
            dashboard_url("https://acme.app.eu2.coralogix.com/", "dash-abc123"),
            "https://acme.app.eu2.coralogix.com/#/dashboards/dash-abc123"
        );
    }

    #[test]
    fn alert_url_joins_base_and_id() {
        assert_eq!(
            alert_url("https://acme.app.eu2.coralogix.com", "alert-xyz789"),
            "https://acme.app.eu2.coralogix.com/#/alerts/alert-xyz789"
        );
    }

    #[test]
    fn alert_url_trims_trailing_slash_on_base() {
        assert_eq!(
            alert_url("https://acme.app.eu2.coralogix.com/", "alert-xyz789"),
            "https://acme.app.eu2.coralogix.com/#/alerts/alert-xyz789"
        );
    }

    #[test]
    fn case_url_uses_query_param_shape() {
        assert_eq!(
            case_url("https://acme.app.eu2.coralogix.com", "case-777"),
            "https://acme.app.eu2.coralogix.com/#/cases?id=case-777"
        );
    }

    #[test]
    fn case_url_percent_encodes_id() {
        // Case readable IDs / raw IDs could contain characters that need
        // encoding in a query string (e.g. spaces, '&', '#').
        assert_eq!(
            case_url("https://acme.app.eu2.coralogix.com", "case #1 & 2"),
            "https://acme.app.eu2.coralogix.com/#/cases?id=case+%231+%26+2"
        );
    }

    #[test]
    fn case_url_trims_trailing_slash_on_base() {
        assert_eq!(
            case_url("https://acme.app.eu2.coralogix.com/", "case-777"),
            "https://acme.app.eu2.coralogix.com/#/cases?id=case-777"
        );
    }

    #[test]
    fn view_url_joins_base_and_id() {
        assert_eq!(
            view_url("https://acme.app.eu2.coralogix.com", "view-123"),
            "https://acme.app.eu2.coralogix.com/#/explore?viewId=view-123"
        );
    }

    #[test]
    fn view_url_percent_encodes_id() {
        assert_eq!(
            view_url("https://acme.app.eu2.coralogix.com", "view #1"),
            "https://acme.app.eu2.coralogix.com/#/explore?viewId=view+%231"
        );
    }

    #[test]
    fn view_url_trims_trailing_slash_on_base() {
        assert_eq!(
            view_url("https://acme.app.eu2.coralogix.com/", "view-123"),
            "https://acme.app.eu2.coralogix.com/#/explore?viewId=view-123"
        );
    }

    #[test]
    fn no_double_slash_when_base_has_trailing_slash() {
        let url = dashboard_url("https://acme.app.eu2.coralogix.com/", "abc");
        assert!(!url.contains("//#/dashboards"));
    }
}
