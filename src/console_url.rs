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
//! instead of the intended entity. Every route below was cross-checked
//! against the console frontend's own routing source
//! (`coralogix/cx-web-workspace`), not just public docs:
//! - Dashboards: `https://<team>.<domain>/#/dashboards/<dashboard_id>` - also
//!   documented at "Share Dashboard URLs",
//!   <https://coralogix.com/docs/user-guides/custom-dashboards/tutorials/share-dashboard-content/>;
//!   confirmed in source (`libs/dashboards/_ui/src/lib/routing-utils.ts`'s
//!   `dashboardsEditUrl()` and the `:id` route under root `dashboards`)
//! - Explore (incl. saved views): `https://<team>.<domain>/#/explore?<params>`,
//!   e.g. `?viewId=<saved_view_id>` - also documented at "Deep links and URL
//!   parameters", <https://coralogix.com/docs/user-guides/data_exploration/deep-links/>;
//!   confirmed in source (`libs/explore/v2/src/lib/services/share-url.service.ts`'s
//!   `viewIdParam()`)
//! - Alerts: `https://<team>.<domain>/#/alerts/<alert_id>` - also documented
//!   pattern for alert deep links referenced from runbooks/webhooks;
//!   confirmed in source (`apps/web-app/src/app/alerts/alerts-routes.ts`,
//!   `:id` child route under `alerts`)
//! - Cases: `https://<team>.<domain>/#/cases?id=<case_id>` - not published in
//!   any public doc, but confirmed in source
//!   (`libs/cases/.../cases-query-params.constants.ts` defines
//!   `SELECTED_CASE_QUERY_PARAM = 'id'`, used by
//!   `insights-incidents-link.service.ts` to build case deep links)
//! - Events2Metrics (`e2m`): `https://<team>.<domain>/#/tco/metrics/<id>` -
//!   confirmed in source (`libs/metrices-settings/src/lib/metrices-settings.component.ts`
//!   calls `location.replaceState('/tco/metrics/' + id)` and reads it back via
//!   `route.params.metricId` to reopen that metric's editor)
//! - SLOs: `https://<team>.<domain>/#/slo/<id>/overview` - confirmed in source
//!   (`libs/slo/src/lib/slo-routes.ts`'s `:sloId/overview` route, navigated to
//!   via `router.navigate(['slo', sloId, 'overview'])` in `slo-page.component.ts`)
//! - Parsing rule groups: `https://<team>.<domain>/#/rules/group/<id>` -
//!   confirmed in source (`libs/rules/src/lib/rule.routes.ts`'s `group/:themeId`
//!   route, navigated to via `router.navigateByUrl('/rules/group/' + group.id)`
//!   in `parsing-theme-list-container.component.ts`)
//! - Alert suppression rules: `https://<team>.<domain>/#/suppression-rules?edit=<id>` -
//!   confirmed in source (`apps/web-app/src/app/features/suppression-rules/...`
//!   uses query field `edit` to reopen a specific rule's editor)
//! - Notification connectors: `https://<team>.<domain>/#/notification-center/connectors?id=<id>` -
//!   confirmed in source (`libs/notification-center/src/lib/features/nc-connectors/...`
//!   reads `queryParams['id']` to auto-open that connector)
//! - Notification routers: `https://<team>.<domain>/#/notification-center/routers?id=<id>` -
//!   confirmed in source (`libs/notification-center/src/lib/features/nc-routers/...`
//!   reads `queryParams['id']` to auto-open that router)
//! - IAM roles: `https://<team>.<domain>/#/settings/roles?selectedRoleId=<id>` -
//!   confirmed in source (`libs/settings/core/.../roles.component.ts` reads
//!   `queryParams['selectedRoleId']` to auto-open that role)
//! - IAM scopes: `https://<team>.<domain>/#/settings/scopes?selectedScopeId=<id>` -
//!   confirmed in source (`libs/settings/core/.../scopes-dashboard.component.ts`
//!   reads `queryParams['selectedScopeId']` to auto-open that scope)
//! - IAM groups: `https://<team>.<domain>/#/settings/account/groups?selectedGroupId=<id>` -
//!   confirmed in source (`libs/settings/core/.../groups-dashboard.component.ts`
//!   reads `queryParams['selectedGroupId']` to auto-open that group)
//!
//! All builders below include the `#/` prefix accordingly. Query parameters
//! on a hash-routed page live *inside* the fragment (`#/path?query=...`),
//! not after a bare path, matching the Explore examples above.

use serde_json::Value;
use url::form_urlencoded;

/// Strip a single trailing `/` from `base`, if present, so callers can join
/// `{base}/#/{path}` without producing a double slash before the hash.
fn trim_base(base: &str) -> &str {
    base.trim_end_matches('/')
}

/// Extract an entity's `id` field from an untyped JSON response as a string,
/// accepting either a JSON string or a JSON number (some APIs return numeric
/// IDs, e.g. IAM roles/scopes/groups).
///
/// Used by callers that only have a raw `serde_json::Value` response (no
/// typed struct with an `id: Option<String>` field) to extract an ID for
/// building a console link.
pub fn id_from_json(val: &Value) -> Option<String> {
    let id = val.get("id")?;
    if let Some(s) = id.as_str() {
        return Some(s.to_string());
    }
    if let Some(n) = id.as_i64() {
        return Some(n.to_string());
    }
    if let Some(n) = id.as_u64() {
        return Some(n.to_string());
    }
    None
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
/// No public doc names the exact query parameter, but this is confirmed
/// directly against the console frontend source
/// (`coralogix/cx-web-workspace`): `SELECTED_CASE_QUERY_PARAM = 'id'` in
/// `libs/cases/.../cases-query-params.constants.ts`, used by
/// `insights-incidents-link.service.ts` to build `/cases?id=<caseId>` links.
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

/// Build the console URL for an E2M (Events2Metrics) definition:
/// `{base}/#/tco/metrics/{id}`.
pub fn e2m_url(base: &str, id: &str) -> String {
    format!("{}/#/tco/metrics/{id}", trim_base(base))
}

/// Build the console URL for an SLO: `{base}/#/slo/{id}/overview`.
pub fn slo_url(base: &str, id: &str) -> String {
    format!("{}/#/slo/{id}/overview", trim_base(base))
}

/// Build the console URL for a parsing rule group: `{base}/#/rules/group/{id}`.
pub fn parsing_rule_group_url(base: &str, id: &str) -> String {
    format!("{}/#/rules/group/{id}", trim_base(base))
}

/// Build the console URL for an alert suppression rule:
/// `{base}/#/suppression-rules?edit={urlencoded id}`.
pub fn suppression_rule_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!("{}/#/suppression-rules?edit={encoded}", trim_base(base))
}

/// Build the console URL for a notification connector:
/// `{base}/#/notification-center/connectors?id={urlencoded id}`.
pub fn notification_connector_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/#/notification-center/connectors?id={encoded}",
        trim_base(base)
    )
}

/// Build the console URL for a notification router:
/// `{base}/#/notification-center/routers?id={urlencoded id}`.
pub fn notification_router_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/#/notification-center/routers?id={encoded}",
        trim_base(base)
    )
}

/// Build the console URL for an IAM role:
/// `{base}/#/settings/roles?selectedRoleId={urlencoded id}`.
pub fn iam_role_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/#/settings/roles?selectedRoleId={encoded}",
        trim_base(base)
    )
}

/// Build the console URL for an IAM scope:
/// `{base}/#/settings/scopes?selectedScopeId={urlencoded id}`.
pub fn iam_scope_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/#/settings/scopes?selectedScopeId={encoded}",
        trim_base(base)
    )
}

/// Build the console URL for an IAM group:
/// `{base}/#/settings/account/groups?selectedGroupId={urlencoded id}`.
pub fn iam_group_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/#/settings/account/groups?selectedGroupId={encoded}",
        trim_base(base)
    )
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

    #[test]
    fn e2m_url_joins_base_and_id() {
        assert_eq!(
            e2m_url("https://acme.app.eu2.coralogix.com", "e2m-123"),
            "https://acme.app.eu2.coralogix.com/#/tco/metrics/e2m-123"
        );
    }

    #[test]
    fn e2m_url_trims_trailing_slash_on_base() {
        assert_eq!(
            e2m_url("https://acme.app.eu2.coralogix.com/", "e2m-123"),
            "https://acme.app.eu2.coralogix.com/#/tco/metrics/e2m-123"
        );
    }

    #[test]
    fn slo_url_joins_base_and_id() {
        assert_eq!(
            slo_url("https://acme.app.eu2.coralogix.com", "slo-abc"),
            "https://acme.app.eu2.coralogix.com/#/slo/slo-abc/overview"
        );
    }

    #[test]
    fn slo_url_trims_trailing_slash_on_base() {
        assert_eq!(
            slo_url("https://acme.app.eu2.coralogix.com/", "slo-abc"),
            "https://acme.app.eu2.coralogix.com/#/slo/slo-abc/overview"
        );
    }

    #[test]
    fn parsing_rule_group_url_joins_base_and_id() {
        assert_eq!(
            parsing_rule_group_url("https://acme.app.eu2.coralogix.com", "group-1"),
            "https://acme.app.eu2.coralogix.com/#/rules/group/group-1"
        );
    }

    #[test]
    fn parsing_rule_group_url_trims_trailing_slash_on_base() {
        assert_eq!(
            parsing_rule_group_url("https://acme.app.eu2.coralogix.com/", "group-1"),
            "https://acme.app.eu2.coralogix.com/#/rules/group/group-1"
        );
    }

    #[test]
    fn suppression_rule_url_uses_query_param_shape() {
        assert_eq!(
            suppression_rule_url("https://acme.app.eu2.coralogix.com", "rule-1"),
            "https://acme.app.eu2.coralogix.com/#/suppression-rules?edit=rule-1"
        );
    }

    #[test]
    fn suppression_rule_url_percent_encodes_id() {
        assert_eq!(
            suppression_rule_url("https://acme.app.eu2.coralogix.com", "rule #1"),
            "https://acme.app.eu2.coralogix.com/#/suppression-rules?edit=rule+%231"
        );
    }

    #[test]
    fn suppression_rule_url_trims_trailing_slash_on_base() {
        assert_eq!(
            suppression_rule_url("https://acme.app.eu2.coralogix.com/", "rule-1"),
            "https://acme.app.eu2.coralogix.com/#/suppression-rules?edit=rule-1"
        );
    }

    #[test]
    fn notification_connector_url_uses_query_param_shape() {
        assert_eq!(
            notification_connector_url("https://acme.app.eu2.coralogix.com", "conn-1"),
            "https://acme.app.eu2.coralogix.com/#/notification-center/connectors?id=conn-1"
        );
    }

    #[test]
    fn notification_connector_url_trims_trailing_slash_on_base() {
        assert_eq!(
            notification_connector_url("https://acme.app.eu2.coralogix.com/", "conn-1"),
            "https://acme.app.eu2.coralogix.com/#/notification-center/connectors?id=conn-1"
        );
    }

    #[test]
    fn notification_router_url_uses_query_param_shape() {
        assert_eq!(
            notification_router_url("https://acme.app.eu2.coralogix.com", "router-1"),
            "https://acme.app.eu2.coralogix.com/#/notification-center/routers?id=router-1"
        );
    }

    #[test]
    fn notification_router_url_trims_trailing_slash_on_base() {
        assert_eq!(
            notification_router_url("https://acme.app.eu2.coralogix.com/", "router-1"),
            "https://acme.app.eu2.coralogix.com/#/notification-center/routers?id=router-1"
        );
    }

    #[test]
    fn iam_role_url_uses_query_param_shape() {
        assert_eq!(
            iam_role_url("https://acme.app.eu2.coralogix.com", "42"),
            "https://acme.app.eu2.coralogix.com/#/settings/roles?selectedRoleId=42"
        );
    }

    #[test]
    fn iam_role_url_trims_trailing_slash_on_base() {
        assert_eq!(
            iam_role_url("https://acme.app.eu2.coralogix.com/", "42"),
            "https://acme.app.eu2.coralogix.com/#/settings/roles?selectedRoleId=42"
        );
    }

    #[test]
    fn iam_scope_url_uses_query_param_shape() {
        assert_eq!(
            iam_scope_url("https://acme.app.eu2.coralogix.com", "scope-1"),
            "https://acme.app.eu2.coralogix.com/#/settings/scopes?selectedScopeId=scope-1"
        );
    }

    #[test]
    fn iam_scope_url_trims_trailing_slash_on_base() {
        assert_eq!(
            iam_scope_url("https://acme.app.eu2.coralogix.com/", "scope-1"),
            "https://acme.app.eu2.coralogix.com/#/settings/scopes?selectedScopeId=scope-1"
        );
    }

    #[test]
    fn iam_group_url_uses_query_param_shape() {
        assert_eq!(
            iam_group_url("https://acme.app.eu2.coralogix.com", "7"),
            "https://acme.app.eu2.coralogix.com/#/settings/account/groups?selectedGroupId=7"
        );
    }

    #[test]
    fn iam_group_url_trims_trailing_slash_on_base() {
        assert_eq!(
            iam_group_url("https://acme.app.eu2.coralogix.com/", "7"),
            "https://acme.app.eu2.coralogix.com/#/settings/account/groups?selectedGroupId=7"
        );
    }

    #[test]
    fn id_from_json_reads_string_id() {
        let val = serde_json::json!({"id": "abc-123"});
        assert_eq!(id_from_json(&val), Some("abc-123".to_string()));
    }

    #[test]
    fn id_from_json_reads_numeric_id() {
        let val = serde_json::json!({"id": 42});
        assert_eq!(id_from_json(&val), Some("42".to_string()));
    }

    #[test]
    fn id_from_json_returns_none_when_missing() {
        let val = serde_json::json!({"name": "no id here"});
        assert_eq!(id_from_json(&val), None);
    }
}
