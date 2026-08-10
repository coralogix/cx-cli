//! Pure builders for "View in Coralogix" web console links.
//!
//! These functions take an already-resolved console base URL (scheme +
//! team subdomain + console domain) and an entity ID, and return the full
//! console URL for that entity.
//!
//! No I/O happens here - resolving the base URL (from an explicit
//! `console_url` override, or otherwise `GET /identity/whoami`) is handled
//! by `ExecutionTarget::console_base` in `crate::execution`. Keeping these
//! as pure string builders makes them trivial to unit test.
//!
//! ## Path routing
//!
//! The Coralogix web console used to route entirely off a `#/` hash
//! fragment, but that was removed (the codebase's own "hash-routing-removal
//! work") in favor of plain path routing via a custom
//! `HostedAppLocationStrategy`
//! (`apps/web-app/src/app/hosted-app-location-strategy.ts` in
//! `coralogix/cx-web-workspace`). Only two carve-outs still serialize onto
//! the fragment: hosted-app routes (`/grafana`, `/opendashboards`, per
//! `HOSTED_APP_HASH_PREFIXES`) and login routes when the session already
//! arrived on a fragment URL (`/login`, `/login-v1`, `/login-v2`, per
//! `AUTH_HASH_PREFIXES`). None of the routes below fall under either
//! carve-out, so they're plain paths - no `#/` prefix. Every route below was
//! cross-checked against the console frontend's own routing source, not just
//! public docs:
//! - Dashboards: `https://<team>.<domain>/dashboards/<dashboard_id>` - also
//!   documented at "Share Dashboard URLs",
//!   <https://coralogix.com/docs/user-guides/custom-dashboards/tutorials/share-dashboard-content/>;
//!   confirmed in source (`libs/dashboards/_ui/src/lib/routing-utils.ts`'s
//!   `dashboardsEditUrl()` and the `:id` route under root `dashboards`)
//! - Explore (incl. saved views): `https://<team>.<domain>/explore?<params>`,
//!   e.g. `?viewId=<saved_view_id>` - also documented at "Deep links and URL
//!   parameters", <https://coralogix.com/docs/user-guides/data_exploration/deep-links/>;
//!   confirmed in source (`libs/explore/v2/src/lib/services/share-url.service.ts`'s
//!   `viewIdParam()`)
//! - Alerts: `https://<team>.<domain>/alerts/<alert_id>` - also documented
//!   pattern for alert deep links referenced from runbooks/webhooks;
//!   confirmed in source (`apps/web-app/src/app/alerts/alerts-routes.ts`,
//!   `:id` child route under `alerts`)
//! - Cases: `https://<team>.<domain>/cases?id=<case_id>` - not published in
//!   any public doc, but confirmed in source
//!   (`libs/cases/.../cases-query-params.constants.ts` defines
//!   `SELECTED_CASE_QUERY_PARAM = 'id'`, used by
//!   `insights-incidents-link.service.ts` to build case deep links)
//! - Events2Metrics (`e2m`): `https://<team>.<domain>/tco/metrics/<id>` -
//!   confirmed in source (`libs/metrices-settings/src/lib/metrices-settings.component.ts`
//!   calls `location.replaceState('/tco/metrics/' + id)` and reads it back via
//!   `route.params.metricId` to reopen that metric's editor)
//! - SLOs: `https://<team>.<domain>/slo/<id>/overview` - confirmed in source
//!   (`libs/slo/src/lib/slo-routes.ts`'s `:sloId/overview` route, navigated to
//!   via `router.navigate(['slo', sloId, 'overview'])` in `slo-page.component.ts`)
//! - Parsing rule groups: `https://<team>.<domain>/rules/group/<id>` -
//!   confirmed in source (`libs/rules/src/lib/rule.routes.ts`'s `group/:themeId`
//!   route, navigated to via `router.navigateByUrl('/rules/group/' + group.id)`
//!   in `parsing-theme-list-container.component.ts`)
//! - Alert suppression rules: `https://<team>.<domain>/suppression-rules?edit=<id>` -
//!   confirmed in source (`apps/web-app/src/app/features/suppression-rules/...`
//!   uses query field `edit` to reopen a specific rule's editor). `<id>` must
//!   be the rule's `uniqueIdentifier`: `suppression-rules.component.ts` resolves
//!   the param via `state.rules.find(rule => rule?.uniqueIdentifier === id)`, so
//!   the API's separate `id` field (the rule *version* id) never matches and the
//!   page quietly drops the param and renders the plain list instead. The
//!   companion `&meta=edit` puts the editor in edit mode - the console always
//!   writes the two together (`updateEditorWithRoute`), and without it the
//!   editor opens titled "New Suppression Rule" with a "Create Rule" button.
//! - Notification connectors: `https://<team>.<domain>/notification-center/connectors?id=<id>` -
//!   confirmed in source (`libs/notification-center/src/lib/features/nc-connectors/...`
//!   reads `queryParams['id']` to auto-open that connector)
//! - Notification routers: `https://<team>.<domain>/notification-center/routers?id=<id>` -
//!   confirmed in source (`libs/notification-center/src/lib/features/nc-routers/...`
//!   reads `queryParams['id']` to auto-open that router)
//! - IAM roles: `https://<team>.<domain>/settings/roles?selectedRoleId=<id>` -
//!   confirmed in source (`libs/settings/core/.../roles.component.ts` reads
//!   `queryParams['selectedRoleId']` to auto-open that role)
//! - IAM scopes: `https://<team>.<domain>/settings/scopes?selectedScopeId=<id>` -
//!   confirmed in source (`libs/settings/core/.../scopes-dashboard.component.ts`
//!   reads `queryParams['selectedScopeId']` to auto-open that scope)
//! - IAM groups: `https://<team>.<domain>/settings/account/groups?selectedGroupId=<id>` -
//!   confirmed in source (`libs/settings/core/.../groups-dashboard.component.ts`
//!   reads `queryParams['selectedGroupId']` to auto-open that group)
//!
//! None of the builders below add a `#/` prefix - query parameters are plain
//! `?query=...` suffixes on the path, matching the Explore examples above.
//!
//! ## Static, per-feature pages (no per-entity ID)
//!
//! A reviewer correctly pointed out that "an entity was created/updated" is
//! not the actual bar for adding a console link - it was just the easiest
//! example, not the rule. Several `cx` command groups map to a single,
//! static settings/report page in the console (there is no per-instance
//! route to deep-link to, but the *feature's page* is still real and worth
//! linking to). These builders take only `base` - no ID:
//! - Usage: `https://<team>.<domain>/settings/datausage` - confirmed in
//!   source (`apps/web-app/src/app/settings/settings-routes.ts`, `path:
//!   'datausage'`, `DataUsageComponent`)
//! - TCO policies: `https://<team>.<domain>/tco-policies` - confirmed in
//!   source (`libs/tco-v2/src/lib/tco-v2-routes.ts`, `path: 'tco-policies'`)
//! - Archive (metrics + logs): `https://<team>.<domain>/physical-locations` -
//!   confirmed in source (`libs/physical-locations/src/lib/physical-locations-routes.ts`,
//!   `path: 'physical-locations'`, backing both the metrics- and
//!   logs-archive handlers in the same lib)
//! - Recording rules: `https://<team>.<domain>/recording-rules` - confirmed
//!   in source (`libs/recording-rules/src/lib/recording-rules-routes.ts`,
//!   `path: 'recording-rules'`)
//! - Enrichments: `https://<team>.<domain>/enrichments` - confirmed in
//!   source (`libs/enrichments/src/lib/enrichments-routes.ts`, `path:
//!   'enrichments'`)
//! - Integrations: `https://<team>.<domain>/extensions/integrations` -
//!   confirmed in source (`app-routes.ts` mounts `extensions/integrations`,
//!   whose empty-path child renders `IntegrationsComponent`,
//!   `data: { title: 'Integrations' }`, from
//!   `libs/integrations/.../integration-routes.ts`)
//! - Webhooks: `https://<team>.<domain>/extensions/all-outbound-webhooks` -
//!   confirmed in source (`app-routes.ts` mounts
//!   `extensions/all-outbound-webhooks`, list page from
//!   `libs/outgoing-webhooks/src/lib/outgoing-webhooks.routes.ts`)
//! - IAM API keys: `https://<team>.<domain>/settings/api-keys` - confirmed
//!   in source (`settings-routes.ts`, `path: 'api-keys'`, `ApiKeysComponent`)
//! - IAM users: `https://<team>.<domain>/settings/team/members` - confirmed
//!   in source (`settings-routes.ts`, `path: 'team/members'`,
//!   `TeamMembersPageComponent`)
//! - IAM IP access: `https://<team>.<domain>/settings/login-access-policies` -
//!   confirmed in source (`settings-routes.ts`, `path:
//!   'login-access-policies'`, `IpAccessComponent`)
//! - AI Center application catalog: `https://<team>.<domain>/ai-center/application-catalog` -
//!   confirmed in source (`apps/web-app/src/app/routes/ai-center-routes.ts`,
//!   `path: 'application-catalog'` as a direct child of `ai-center`,
//!   `CxaiApplicationCatalogComponent`). Note `application-catalog` is a
//!   sibling of `overview`, not nested under it.
//! - AI Center single application: `https://<team>.<domain>/ai-center/application/drilldown?application=<app>&subsystem=<subsystem>` -
//!   confirmed in source (same file, the `application` branch with the
//!   `drilldown` default child; the app is identified by the `application` and
//!   `subsystem` query params from `CXAiApplicationQueryParams` in
//!   `libs/ai-center/root/src/lib/utils.ts`, as used by the catalog grid's
//!   `router.navigate(['ai-center/application'], { queryParams: {...} })`)
//! - AI Center evaluations / policies: `https://<team>.<domain>/ai-center/eval-catalog` -
//!   confirmed in source (same file, `path: 'eval-catalog'` as a direct child
//!   of `ai-center`, `EvalCatalogMultiAppComponent`; the sidenav labels this
//!   the "Policy Catalog")
//! - Olly: `https://<team>.<domain>/olly` - confirmed in source
//!   (`libs/olly/src/lib/olly.routes.ts`, `path: 'olly'`)

use serde_json::Value;
use url::form_urlencoded;

/// Strip a single trailing `/` from `base`, if present, so callers can join
/// `{base}/{path}` without producing a double slash.
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

/// Build the console URL for a dashboard: `{base}/dashboards/{id}`.
pub fn dashboard_url(base: &str, id: &str) -> String {
    format!("{}/dashboards/{id}", trim_base(base))
}

/// Build the console URL for an alert: `{base}/alerts/{id}`.
pub fn alert_url(base: &str, id: &str) -> String {
    format!("{}/alerts/{id}", trim_base(base))
}

/// Build the console URL for the alerts list page: `{base}/alerts`.
pub fn alerts_url(base: &str) -> String {
    format!("{}/alerts", trim_base(base))
}

/// Build the console URL for a case: `{base}/cases?id={urlencoded id}`.
///
/// No public doc names the exact query parameter, but this is confirmed
/// directly against the console frontend source
/// (`coralogix/cx-web-workspace`): `SELECTED_CASE_QUERY_PARAM = 'id'` in
/// `libs/cases/.../cases-query-params.constants.ts`, used by
/// `insights-incidents-link.service.ts` to build `/cases?id=<caseId>` links.
pub fn case_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!("{}/cases?id={encoded}", trim_base(base))
}

/// Build the console URL for a saved Explore view: `{base}/explore?viewId={id}`.
///
/// `cx views` manages the same "saved view" entity referenced by Explore's
/// documented `viewId` deep-link parameter (both are backed by the
/// `data-exploration/views` API namespace) - see "Deep links and URL
/// parameters", <https://coralogix.com/docs/user-guides/data_exploration/deep-links/>.
pub fn view_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!("{}/explore?viewId={encoded}", trim_base(base))
}

/// Build the console URL for an E2M (Events2Metrics) definition:
/// `{base}/tco/metrics/{id}`.
pub fn e2m_url(base: &str, id: &str) -> String {
    format!("{}/tco/metrics/{id}", trim_base(base))
}

/// Build the console URL for an SLO: `{base}/slo/{id}/overview`.
pub fn slo_url(base: &str, id: &str) -> String {
    format!("{}/slo/{id}/overview", trim_base(base))
}

/// Build the console URL for an alert suppression rule:
/// `{base}/suppression-rules?edit={urlencoded id}&meta=edit`.
///
/// `id` must be the rule's `uniqueIdentifier`, not the API's `id` field - the
/// latter is the rule *version* id, which the console's `?edit=` lookup
/// (`rules.find(rule => rule?.uniqueIdentifier === id)`) never matches, leaving
/// the user on the bare list page.
///
/// `meta=edit` is what puts the editor in edit mode. The console always emits
/// the pair (`updateEditorWithRoute` writes both query params), and the editor
/// header keys off it - `isEditMode()` is `operation() === 'edit'`. Omitting it
/// still opens the right rule, but titled "New Suppression Rule" with a "Create
/// Rule" button, so saving would duplicate the rule instead of updating it.
pub fn suppression_rule_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/suppression-rules?edit={encoded}&meta=edit",
        trim_base(base)
    )
}

/// Build the console URL for the suppression rules list page:
/// `{base}/suppression-rules`.
pub fn suppression_rules_url(base: &str) -> String {
    format!("{}/suppression-rules", trim_base(base))
}

/// Build the console URL for the notification connectors list page:
/// `{base}/notification-center/connectors`.
pub fn notification_connectors_url(base: &str) -> String {
    format!("{}/notification-center/connectors", trim_base(base))
}

/// Build the console URL for a notification connector:
/// `{base}/notification-center/connectors?id={urlencoded id}`.
pub fn notification_connector_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/notification-center/connectors?id={encoded}",
        trim_base(base)
    )
}

/// Build the console URL for a notification router:
/// `{base}/notification-center/routers?id={urlencoded id}`.
pub fn notification_router_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/notification-center/routers?id={encoded}",
        trim_base(base)
    )
}

/// Build the console URL for an IAM role:
/// `{base}/settings/roles?selectedRoleId={urlencoded id}`.
pub fn iam_role_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/settings/roles?selectedRoleId={encoded}",
        trim_base(base)
    )
}

/// Build the console URL for an IAM scope:
/// `{base}/settings/scopes?selectedScopeId={urlencoded id}`.
pub fn iam_scope_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/settings/scopes?selectedScopeId={encoded}",
        trim_base(base)
    )
}

/// Build the console URL for an IAM group:
/// `{base}/settings/account/groups?selectedGroupId={urlencoded id}`.
pub fn iam_group_url(base: &str, id: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();
    format!(
        "{}/settings/account/groups?selectedGroupId={encoded}",
        trim_base(base)
    )
}

/// Build the console URL for the Usage page: `{base}/settings/datausage`.
///
/// Static, per-team page - no per-entity ID. `cx usage` is read-only
/// reporting, but a real console page exists for it, so it still earns a
/// link (per reviewer feedback: "an entity was created" was never the rule).
pub fn usage_url(base: &str) -> String {
    format!("{}/settings/datausage", trim_base(base))
}

/// Build the console URL for the TCO policies page: `{base}/tco-policies`.
pub fn tco_url(base: &str) -> String {
    format!("{}/tco-policies", trim_base(base))
}

/// Build the console URL for the Archive (metrics + logs) settings page:
/// `{base}/physical-locations`.
pub fn archive_url(base: &str) -> String {
    format!("{}/physical-locations", trim_base(base))
}

/// Build the console URL for the Recording Rules page:
/// `{base}/recording-rules`.
pub fn recording_rules_url(base: &str) -> String {
    format!("{}/recording-rules", trim_base(base))
}

/// Build the console URL for the Enrichments page: `{base}/enrichments`.
pub fn enrichments_url(base: &str) -> String {
    format!("{}/enrichments", trim_base(base))
}

/// Build the console URL for the Integrations list page:
/// `{base}/extensions/integrations`.
pub fn integrations_url(base: &str) -> String {
    format!("{}/extensions/integrations", trim_base(base))
}

/// Build the console URL for the Outgoing Webhooks list page:
/// `{base}/extensions/all-outbound-webhooks`.
pub fn webhooks_url(base: &str) -> String {
    format!("{}/extensions/all-outbound-webhooks", trim_base(base))
}

/// Build the console URL for the IAM API keys settings page:
/// `{base}/settings/api-keys`.
pub fn iam_api_keys_url(base: &str) -> String {
    format!("{}/settings/api-keys", trim_base(base))
}

/// Build the console URL for the IAM users (team members) settings page:
/// `{base}/settings/team/members`.
pub fn iam_users_url(base: &str) -> String {
    format!("{}/settings/team/members", trim_base(base))
}

/// Build the console URL for the IAM IP access settings page:
/// `{base}/settings/login-access-policies`.
pub fn iam_ip_access_url(base: &str) -> String {
    format!("{}/settings/login-access-policies", trim_base(base))
}

/// Build the console URL for the AI Center application catalog page:
/// `{base}/ai-center/application-catalog`.
pub fn ai_center_applications_url(base: &str) -> String {
    format!("{}/ai-center/application-catalog", trim_base(base))
}

/// Build the console URL for a single AI Center application:
/// `{base}/ai-center/application/drilldown?application={app}&subsystem={sub}`.
///
/// The application is identified by the `application` and `subsystem` query
/// params (there is no path-param route for a single application in the
/// console) - see `CXAiApplicationQueryParams` in
/// `libs/ai-center/root/src/lib/utils.ts`.
pub fn ai_center_application_url(base: &str, application: &str, subsystem: &str) -> String {
    let app: String = form_urlencoded::byte_serialize(application.as_bytes()).collect();
    let sub: String = form_urlencoded::byte_serialize(subsystem.as_bytes()).collect();
    format!(
        "{}/ai-center/application/drilldown?application={app}&subsystem={sub}",
        trim_base(base)
    )
}

/// Build the console URL for the AI Center evaluation (policy) catalog page:
/// `{base}/ai-center/eval-catalog`.
pub fn ai_center_evaluations_url(base: &str) -> String {
    format!("{}/ai-center/eval-catalog", trim_base(base))
}

/// Build the console URL for the Olly AI assistant page: `{base}/olly`.
pub fn olly_url(base: &str) -> String {
    format!("{}/olly", trim_base(base))
}

/// Build the console URL for a specific Olly chat: `{base}/olly/chat/{chat_id}`.
pub fn olly_chat_url(base: &str, chat_id: &str) -> String {
    format!("{}/olly/chat/{}", trim_base(base), chat_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_url_joins_base_and_id() {
        assert_eq!(
            dashboard_url("https://c4c.app.eu2.coralogix.com", "dash-abc123"),
            "https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123"
        );
    }

    #[test]
    fn dashboard_url_trims_trailing_slash_on_base() {
        assert_eq!(
            dashboard_url("https://c4c.app.eu2.coralogix.com/", "dash-abc123"),
            "https://c4c.app.eu2.coralogix.com/dashboards/dash-abc123"
        );
    }

    #[test]
    fn alert_url_joins_base_and_id() {
        assert_eq!(
            alert_url("https://c4c.app.eu2.coralogix.com", "alert-xyz789"),
            "https://c4c.app.eu2.coralogix.com/alerts/alert-xyz789"
        );
    }

    #[test]
    fn alert_url_trims_trailing_slash_on_base() {
        assert_eq!(
            alert_url("https://c4c.app.eu2.coralogix.com/", "alert-xyz789"),
            "https://c4c.app.eu2.coralogix.com/alerts/alert-xyz789"
        );
    }

    #[test]
    fn case_url_uses_query_param_shape() {
        assert_eq!(
            case_url("https://c4c.app.eu2.coralogix.com", "case-777"),
            "https://c4c.app.eu2.coralogix.com/cases?id=case-777"
        );
    }

    #[test]
    fn case_url_percent_encodes_id() {
        // Case readable IDs / raw IDs could contain characters that need
        // encoding in a query string (e.g. spaces, '&', '#').
        assert_eq!(
            case_url("https://c4c.app.eu2.coralogix.com", "case #1 & 2"),
            "https://c4c.app.eu2.coralogix.com/cases?id=case+%231+%26+2"
        );
    }

    #[test]
    fn case_url_trims_trailing_slash_on_base() {
        assert_eq!(
            case_url("https://c4c.app.eu2.coralogix.com/", "case-777"),
            "https://c4c.app.eu2.coralogix.com/cases?id=case-777"
        );
    }

    #[test]
    fn view_url_joins_base_and_id() {
        assert_eq!(
            view_url("https://c4c.app.eu2.coralogix.com", "view-123"),
            "https://c4c.app.eu2.coralogix.com/explore?viewId=view-123"
        );
    }

    #[test]
    fn view_url_percent_encodes_id() {
        assert_eq!(
            view_url("https://c4c.app.eu2.coralogix.com", "view #1"),
            "https://c4c.app.eu2.coralogix.com/explore?viewId=view+%231"
        );
    }

    #[test]
    fn view_url_trims_trailing_slash_on_base() {
        assert_eq!(
            view_url("https://c4c.app.eu2.coralogix.com/", "view-123"),
            "https://c4c.app.eu2.coralogix.com/explore?viewId=view-123"
        );
    }

    #[test]
    fn no_double_slash_when_base_has_trailing_slash() {
        let url = dashboard_url("https://c4c.app.eu2.coralogix.com/", "abc");
        assert!(!url.contains("//dashboards"));
    }

    #[test]
    fn e2m_url_joins_base_and_id() {
        assert_eq!(
            e2m_url("https://c4c.app.eu2.coralogix.com", "e2m-123"),
            "https://c4c.app.eu2.coralogix.com/tco/metrics/e2m-123"
        );
    }

    #[test]
    fn e2m_url_trims_trailing_slash_on_base() {
        assert_eq!(
            e2m_url("https://c4c.app.eu2.coralogix.com/", "e2m-123"),
            "https://c4c.app.eu2.coralogix.com/tco/metrics/e2m-123"
        );
    }

    #[test]
    fn slo_url_joins_base_and_id() {
        assert_eq!(
            slo_url("https://c4c.app.eu2.coralogix.com", "slo-abc"),
            "https://c4c.app.eu2.coralogix.com/slo/slo-abc/overview"
        );
    }

    #[test]
    fn slo_url_trims_trailing_slash_on_base() {
        assert_eq!(
            slo_url("https://c4c.app.eu2.coralogix.com/", "slo-abc"),
            "https://c4c.app.eu2.coralogix.com/slo/slo-abc/overview"
        );
    }

    #[test]
    fn suppression_rule_url_uses_query_param_shape() {
        assert_eq!(
            suppression_rule_url("https://c4c.app.eu2.coralogix.com", "rule-1"),
            "https://c4c.app.eu2.coralogix.com/suppression-rules?edit=rule-1&meta=edit"
        );
    }

    #[test]
    fn suppression_rule_url_percent_encodes_id() {
        assert_eq!(
            suppression_rule_url("https://c4c.app.eu2.coralogix.com", "rule #1"),
            "https://c4c.app.eu2.coralogix.com/suppression-rules?edit=rule+%231&meta=edit"
        );
    }

    #[test]
    fn suppression_rule_url_trims_trailing_slash_on_base() {
        assert_eq!(
            suppression_rule_url("https://c4c.app.eu2.coralogix.com/", "rule-1"),
            "https://c4c.app.eu2.coralogix.com/suppression-rules?edit=rule-1&meta=edit"
        );
    }

    #[test]
    fn suppression_rules_url_is_static() {
        assert_eq!(
            suppression_rules_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/suppression-rules"
        );
    }

    #[test]
    fn suppression_rules_url_trims_trailing_slash_on_base() {
        assert_eq!(
            suppression_rules_url("https://c4c.app.eu2.coralogix.com/"),
            "https://c4c.app.eu2.coralogix.com/suppression-rules"
        );
    }

    #[test]
    fn notification_connectors_url_is_static() {
        assert_eq!(
            notification_connectors_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/notification-center/connectors"
        );
    }

    #[test]
    fn notification_connectors_url_trims_trailing_slash_on_base() {
        assert_eq!(
            notification_connectors_url("https://c4c.app.eu2.coralogix.com/"),
            "https://c4c.app.eu2.coralogix.com/notification-center/connectors"
        );
    }

    #[test]
    fn notification_connector_url_uses_query_param_shape() {
        assert_eq!(
            notification_connector_url("https://c4c.app.eu2.coralogix.com", "conn-1"),
            "https://c4c.app.eu2.coralogix.com/notification-center/connectors?id=conn-1"
        );
    }

    #[test]
    fn notification_connector_url_trims_trailing_slash_on_base() {
        assert_eq!(
            notification_connector_url("https://c4c.app.eu2.coralogix.com/", "conn-1"),
            "https://c4c.app.eu2.coralogix.com/notification-center/connectors?id=conn-1"
        );
    }

    #[test]
    fn notification_router_url_uses_query_param_shape() {
        assert_eq!(
            notification_router_url("https://c4c.app.eu2.coralogix.com", "router-1"),
            "https://c4c.app.eu2.coralogix.com/notification-center/routers?id=router-1"
        );
    }

    #[test]
    fn notification_router_url_trims_trailing_slash_on_base() {
        assert_eq!(
            notification_router_url("https://c4c.app.eu2.coralogix.com/", "router-1"),
            "https://c4c.app.eu2.coralogix.com/notification-center/routers?id=router-1"
        );
    }

    #[test]
    fn iam_role_url_uses_query_param_shape() {
        assert_eq!(
            iam_role_url("https://c4c.app.eu2.coralogix.com", "42"),
            "https://c4c.app.eu2.coralogix.com/settings/roles?selectedRoleId=42"
        );
    }

    #[test]
    fn iam_role_url_trims_trailing_slash_on_base() {
        assert_eq!(
            iam_role_url("https://c4c.app.eu2.coralogix.com/", "42"),
            "https://c4c.app.eu2.coralogix.com/settings/roles?selectedRoleId=42"
        );
    }

    #[test]
    fn iam_scope_url_uses_query_param_shape() {
        assert_eq!(
            iam_scope_url("https://c4c.app.eu2.coralogix.com", "scope-1"),
            "https://c4c.app.eu2.coralogix.com/settings/scopes?selectedScopeId=scope-1"
        );
    }

    #[test]
    fn iam_scope_url_trims_trailing_slash_on_base() {
        assert_eq!(
            iam_scope_url("https://c4c.app.eu2.coralogix.com/", "scope-1"),
            "https://c4c.app.eu2.coralogix.com/settings/scopes?selectedScopeId=scope-1"
        );
    }

    #[test]
    fn iam_group_url_uses_query_param_shape() {
        assert_eq!(
            iam_group_url("https://c4c.app.eu2.coralogix.com", "7"),
            "https://c4c.app.eu2.coralogix.com/settings/account/groups?selectedGroupId=7"
        );
    }

    #[test]
    fn iam_group_url_trims_trailing_slash_on_base() {
        assert_eq!(
            iam_group_url("https://c4c.app.eu2.coralogix.com/", "7"),
            "https://c4c.app.eu2.coralogix.com/settings/account/groups?selectedGroupId=7"
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

    #[test]
    fn usage_url_is_static() {
        assert_eq!(
            usage_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/settings/datausage"
        );
    }

    #[test]
    fn usage_url_trims_trailing_slash_on_base() {
        assert_eq!(
            usage_url("https://c4c.app.eu2.coralogix.com/"),
            "https://c4c.app.eu2.coralogix.com/settings/datausage"
        );
    }

    #[test]
    fn tco_url_is_static() {
        assert_eq!(
            tco_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/tco-policies"
        );
    }

    #[test]
    fn archive_url_is_static() {
        assert_eq!(
            archive_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/physical-locations"
        );
    }

    #[test]
    fn recording_rules_url_is_static() {
        assert_eq!(
            recording_rules_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/recording-rules"
        );
    }

    #[test]
    fn enrichments_url_is_static() {
        assert_eq!(
            enrichments_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/enrichments"
        );
    }

    #[test]
    fn integrations_url_is_static() {
        assert_eq!(
            integrations_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/extensions/integrations"
        );
    }

    #[test]
    fn webhooks_url_is_static() {
        assert_eq!(
            webhooks_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/extensions/all-outbound-webhooks"
        );
    }

    #[test]
    fn iam_api_keys_url_is_static() {
        assert_eq!(
            iam_api_keys_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/settings/api-keys"
        );
    }

    #[test]
    fn iam_users_url_is_static() {
        assert_eq!(
            iam_users_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/settings/team/members"
        );
    }

    #[test]
    fn iam_ip_access_url_is_static() {
        assert_eq!(
            iam_ip_access_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/settings/login-access-policies"
        );
    }

    #[test]
    fn ai_center_applications_url_is_static() {
        assert_eq!(
            ai_center_applications_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/ai-center/application-catalog"
        );
    }

    #[test]
    fn ai_center_application_url_uses_query_params() {
        assert_eq!(
            ai_center_application_url("https://c4c.app.eu2.coralogix.com", "checkout", "payments"),
            "https://c4c.app.eu2.coralogix.com/ai-center/application/drilldown?application=checkout&subsystem=payments"
        );
    }

    #[test]
    fn ai_center_application_url_percent_encodes_params() {
        assert_eq!(
            ai_center_application_url("https://c4c.app.eu2.coralogix.com", "my app", "sub & sys"),
            "https://c4c.app.eu2.coralogix.com/ai-center/application/drilldown?application=my+app&subsystem=sub+%26+sys"
        );
    }

    #[test]
    fn ai_center_application_url_trims_trailing_slash_on_base() {
        assert_eq!(
            ai_center_application_url("https://c4c.app.eu2.coralogix.com/", "checkout", "payments"),
            "https://c4c.app.eu2.coralogix.com/ai-center/application/drilldown?application=checkout&subsystem=payments"
        );
    }

    #[test]
    fn ai_center_evaluations_url_is_static() {
        assert_eq!(
            ai_center_evaluations_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/ai-center/eval-catalog"
        );
    }

    #[test]
    fn olly_url_is_static() {
        assert_eq!(
            olly_url("https://c4c.app.eu2.coralogix.com"),
            "https://c4c.app.eu2.coralogix.com/olly"
        );
    }

    #[test]
    fn olly_chat_url_includes_chat_id() {
        assert_eq!(
            olly_chat_url(
                "https://c4c.app.eu2.coralogix.com",
                "1a58088c-aff2-46aa-b5b5-f6e109d7bc3f"
            ),
            "https://c4c.app.eu2.coralogix.com/olly/chat/1a58088c-aff2-46aa-b5b5-f6e109d7bc3f"
        );
    }

    #[test]
    fn static_urls_trim_trailing_slash_on_base() {
        assert_eq!(
            tco_url("https://c4c.app.eu2.coralogix.com/"),
            "https://c4c.app.eu2.coralogix.com/tco-policies"
        );
        assert_eq!(
            olly_url("https://c4c.app.eu2.coralogix.com/"),
            "https://c4c.app.eu2.coralogix.com/olly"
        );
    }
}
