use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Query search response types ---

#[derive(Debug, Serialize, Deserialize)]
pub struct QuerySearchResult {
    pub query_text: String,
    pub similarity: f64,
    pub dashboard_name: Option<String>,
    pub dashboard_folder: Option<String>,
    pub widget_title: Option<String>,
    pub widget_type: Option<String>,
    pub query_context: Option<String>,
    #[serde(default)]
    pub extracted_fields: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct QuerySearchResponse {
    pub results: Vec<QuerySearchResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryByFieldResult {
    pub query_text: String,
    pub dashboard_name: Option<String>,
    pub widget_title: Option<String>,
    #[serde(default)]
    pub matched_fields: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct QueryByFieldResponse {
    pub queries: Vec<QueryByFieldResult>,
}

// --- Dashboard semantic search response types ---

#[derive(Debug, Serialize, Deserialize)]
pub struct DashboardSearchResult {
    pub dashboard_id: String,
    pub dashboard_name: Option<String>,
    pub dashboard_folder: Option<String>,
    pub description: Option<String>,
    pub semantic_description: Option<String>,
    #[serde(default)]
    pub widget_count: Option<u32>,
    pub similarity: f64,
}

#[derive(Debug, Deserialize)]
pub struct DashboardSemanticSearchResponse {
    pub results: Vec<DashboardSearchResult>,
}

// --- Catalog response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardFolder {
    pub id: Option<String>,
    pub name: Option<String>,
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardCatalogItem {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub slug_name: Option<String>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
    pub is_default: Option<bool>,
    pub is_pinned: Option<bool>,
    pub is_locked: Option<bool>,
    pub folder: Option<DashboardFolder>,
}

#[derive(Debug, Deserialize)]
pub struct DashboardCatalogResponse {
    pub items: Vec<DashboardCatalogItem>,
}

// --- Folders response types ---

/// The folders API returns each item with plain string `id` / `parentId`
/// (no `{"value": "..."}` wrapper), and the top-level array is named `folder`
/// (singular) rather than `folders`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardFolderItem {
    pub id: Option<String>,
    pub name: Option<String>,
    pub parent_id: Option<String>,
}

impl DashboardFolderItem {
    pub fn id_str(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn parent_id_str(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }
}

#[derive(Debug, Deserialize)]
pub struct DashboardFoldersResponse {
    #[serde(default, rename = "folder")]
    pub folders: Vec<DashboardFolderItem>,
}

// --- Dashboard check (CheckDashboard) response types ---

/// Severity of a validation issue emitted by `CheckDashboard`.
///
/// The protobuf JSON gateway emits enum values as their SCREAMING_SNAKE_CASE
/// names (`"SEVERITY_ERROR"`, `"SEVERITY_WARNING"`, `"SEVERITY_UNSPECIFIED"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueSeverity {
    SeverityUnspecified,
    SeverityError,
    SeverityWarning,
}

impl IssueSeverity {
    /// True when this severity should fail a `check` invocation (CI gate).
    /// Today only `SEVERITY_ERROR` (and the unspecified default, treated as
    /// an error to be safe) cause a non-zero exit; warnings print but pass.
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            IssueSeverity::SeverityError | IssueSeverity::SeverityUnspecified
        )
    }
}

/// A single validation issue returned by `CheckDashboard`.
///
/// `location` is an RFC 6901 JSON Pointer into the dashboard definition
/// (e.g. `/sections/0/rows/1/widgets/2`); empty/omitted for root-level issues.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardCheckIssue {
    pub severity: IssueSeverity,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
}

/// Response body of `POST /dashboards/check/v1`.
///
/// An empty `issues` list means the dashboard is valid.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckDashboardResponse {
    #[serde(default)]
    pub issues: Vec<DashboardCheckIssue>,
}

// --- API ---

const DASHBOARDS_BASE: &str = "/mgmt/openapi/5/dashboards/dashboards/v1";
const FOLDERS_BASE: &str = "/mgmt/openapi/5/dashboards/folders/v1";
/// `POST`: validate a dashboard definition or an existing dashboard by id
/// without persisting it (DashboardsService.CheckDashboard).
///
/// The proto HTTP annotation is `POST /dashboards/check/v1` (body: `*`);
/// the management gateway maps the `/dashboards` prefix to
/// `/mgmt/openapi/5/dashboards`.
const DASHBOARD_CHECK_PATH: &str = "/mgmt/openapi/5/dashboards/check/v1";

#[derive(Debug, Deserialize)]
pub struct DeleteDashboardResponse {}

#[derive(Debug, Deserialize)]
pub struct DeleteDashboardFolderResponse {}

/// `GET`: natural-language search over saved dashboard queries/widgets.
/// Public platform path (gateway `olly-kb` prefix → service `/api/v1/dashboards/...`).
const QUERIES_SEARCH_PATH: &str = "/api/v1/olly-kb/dashboards/queries/search";
/// `GET`: semantic search over dashboard metadata.
const DASHBOARDS_SEMANTIC_SEARCH_PATH: &str = "/api/v1/olly-kb/dashboards/semantic-search";
/// `GET`: list queries that reference a DataPrime field path.
const QUERIES_BY_FIELD_PATH: &str = "/api/v1/olly-kb/queries/by-field";

const SEMANTIC_QUERY_LIMIT_MIN: u32 = 1;
const SEMANTIC_QUERY_LIMIT_MAX: u32 = 100;

/// JSON body / GET query key: natural-language query text.
const REQ_KEY_QUERY_TEXT: &str = "query_text";
/// GET query key: DataPrime-style field path (`queries-by-field`).
const REQ_KEY_FIELD_PATH: &str = "field_path";
/// JSON body / GET query key: maximum number of results.
const REQ_KEY_LIMIT: &str = "limit";

#[inline]
fn clamp_semantic_query_limit(limit: u32) -> u32 {
    limit.clamp(SEMANTIC_QUERY_LIMIT_MIN, SEMANTIC_QUERY_LIMIT_MAX)
}

pub struct DashboardsApi<'a> {
    client: &'a CxClient,
}

impl<'a> DashboardsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// List all dashboards in the catalog.
    pub async fn catalog(&self) -> Result<DashboardCatalogResponse> {
        self.client
            .get(&format!("{DASHBOARDS_BASE}/catalog/list"), &[])
            .await
    }

    /// Get a single dashboard by ID (returns raw JSON - the schema is large).
    pub async fn get(&self, id: &str) -> Result<Value> {
        self.client
            .get(&format!("{DASHBOARDS_BASE}/{id}"), &[])
            .await
    }

    /// Create a new dashboard. `body` must be the full
    /// `{ "requestId": ..., "dashboard": { ... } }` payload expected by the
    /// Coralogix Dashboard Service.
    pub async fn create(&self, body: &Value) -> Result<Value> {
        self.client.post(DASHBOARDS_BASE, body).await
    }

    /// Validate a dashboard definition without persisting it
    /// (`DashboardsService.CheckDashboard`).
    ///
    /// `body` must be a `CheckDashboardRequest` JSON object with exactly one
    /// of these `source` oneof fields set:
    /// - `{ "dashboard": { ... } }` — validate an inline definition
    /// - `{ "dashboardId": "<id>" }` — validate a stored dashboard by id
    pub async fn check(&self, body: &Value) -> Result<CheckDashboardResponse> {
        self.client.post(DASHBOARD_CHECK_PATH, body).await
    }

    /// List all dashboard folders.
    pub async fn folders(&self) -> Result<DashboardFoldersResponse> {
        self.client.get(FOLDERS_BASE, &[]).await
    }

    /// Create a new dashboard folder. `body` must be the
    /// `{ "requestId": ..., "folder": { "name": ..., "parentId": ... } }`
    /// payload expected by the Dashboard Folders Service.
    pub async fn folders_create(&self, body: &Value) -> Result<Value> {
        self.client.post(FOLDERS_BASE, body).await
    }

    /// Replace an existing dashboard. `body` must be the full
    /// `{ "requestId": ..., "dashboard": { ... } }` payload with the dashboard
    /// `id` set to the target dashboard.
    pub async fn replace(&self, body: &Value) -> Result<Value> {
        self.client.put(DASHBOARDS_BASE, body).await
    }

    /// Delete a dashboard by ID.
    pub async fn delete(&self, id: &str) -> Result<DeleteDashboardResponse> {
        let path = format!("{DASHBOARDS_BASE}/{id}");
        self.client.delete(&path).await
    }

    /// Delete a dashboard folder by ID.
    pub async fn folders_delete(&self, id: &str) -> Result<DeleteDashboardFolderResponse> {
        let path = format!("{FOLDERS_BASE}/{id}");
        self.client.delete(&path).await
    }

    /// Semantic search over dashboard queries/widgets.
    pub async fn search_queries(
        &self,
        query_text: &str,
        limit: u32,
    ) -> Result<QuerySearchResponse> {
        let limit = clamp_semantic_query_limit(limit);
        let limit_str = limit.to_string();
        self.client
            .get(
                QUERIES_SEARCH_PATH,
                &[
                    (REQ_KEY_QUERY_TEXT, query_text),
                    (REQ_KEY_LIMIT, limit_str.as_str()),
                ],
            )
            .await
    }

    /// Semantic search over dashboards by natural-language query.
    pub async fn semantic_search(
        &self,
        query_text: &str,
        limit: u32,
    ) -> Result<DashboardSemanticSearchResponse> {
        let limit = clamp_semantic_query_limit(limit);
        let limit_str = limit.to_string();
        self.client
            .get(
                DASHBOARDS_SEMANTIC_SEARCH_PATH,
                &[
                    (REQ_KEY_QUERY_TEXT, query_text),
                    (REQ_KEY_LIMIT, limit_str.as_str()),
                ],
            )
            .await
    }

    /// Find all dashboard queries that reference a specific field path.
    pub async fn queries_by_field(
        &self,
        field_path: &str,
        limit: u32,
    ) -> Result<QueryByFieldResponse> {
        let limit = clamp_semantic_query_limit(limit);
        let limit_str = limit.to_string();
        self.client
            .get(
                QUERIES_BY_FIELD_PATH,
                &[
                    (REQ_KEY_FIELD_PATH, field_path),
                    (REQ_KEY_LIMIT, limit_str.as_str()),
                ],
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_response_deserializes_mixed_severity_issues() {
        let raw = serde_json::json!({
            "issues": [
                {
                    "severity": "SEVERITY_ERROR",
                    "message": "Widget 'cpu-chart' references undefined variable 'env'",
                    "location": "/sections/0/rows/1/widgets/2"
                },
                {
                    "severity": "SEVERITY_WARNING",
                    "message": "Query uses deprecated function 'timeShift'",
                    "location": "/sections/1/rows/0/widgets/0/queries/0"
                }
            ]
        });
        let resp: CheckDashboardResponse = serde_json::from_value(raw).expect("must deserialize");
        assert_eq!(resp.issues.len(), 2);
        assert_eq!(resp.issues[0].severity, IssueSeverity::SeverityError);
        assert_eq!(
            resp.issues[0].message.as_deref(),
            Some("Widget 'cpu-chart' references undefined variable 'env'")
        );
        assert_eq!(
            resp.issues[0].location.as_deref(),
            Some("/sections/0/rows/1/widgets/2")
        );
        assert_eq!(resp.issues[1].severity, IssueSeverity::SeverityWarning);
        assert!(!resp.issues[1].severity.is_failure());

        // The error-severity issue must be a failure, the warning must not.
        assert!(resp.issues[0].severity.is_failure());
        assert!(!resp.issues[1].severity.is_failure());
    }

    #[test]
    fn check_response_empty_issues_is_valid() {
        let raw = serde_json::json!({ "issues": [] });
        let resp: CheckDashboardResponse = serde_json::from_value(raw).expect("must deserialize");
        assert!(resp.issues.is_empty());
    }

    #[test]
    fn check_response_defaults_issues_to_empty_when_absent() {
        // The server always emits `issues`, but `#[serde(default)]` should
        // make a missing field non-fatal.
        let raw = serde_json::json!({});
        let resp: CheckDashboardResponse = serde_json::from_value(raw).expect("must deserialize");
        assert!(resp.issues.is_empty());
    }

    #[test]
    fn check_response_tolerates_issue_missing_optional_fields() {
        let raw = serde_json::json!({
            "issues": [
                { "severity": "SEVERITY_ERROR" },
                { "severity": "SEVERITY_WARNING", "message": "only a message" },
                { "severity": "SEVERITY_UNSPECIFIED", "location": "/only/location" }
            ]
        });
        let resp: CheckDashboardResponse = serde_json::from_value(raw).expect("must deserialize");
        assert_eq!(resp.issues.len(), 3);

        assert_eq!(resp.issues[0].severity, IssueSeverity::SeverityError);
        assert!(resp.issues[0].message.is_none());
        assert!(resp.issues[0].location.is_none());

        assert_eq!(resp.issues[1].severity, IssueSeverity::SeverityWarning);
        assert_eq!(resp.issues[1].message.as_deref(), Some("only a message"));
        assert!(resp.issues[1].location.is_none());

        assert_eq!(resp.issues[2].severity, IssueSeverity::SeverityUnspecified);
        assert!(resp.issues[2].message.is_none());
        assert_eq!(resp.issues[2].location.as_deref(), Some("/only/location"));
        // Unspecified is treated as a failure to be safe.
        assert!(resp.issues[2].severity.is_failure());
    }

    #[test]
    fn issue_severity_serializes_to_screaming_snake_case() {
        // Round-trip: the enum must serialize back to the proto JSON shape
        // so it can be re-emitted in --json / --toon output.
        for (sev, expected) in [
            (IssueSeverity::SeverityUnspecified, "SEVERITY_UNSPECIFIED"),
            (IssueSeverity::SeverityError, "SEVERITY_ERROR"),
            (IssueSeverity::SeverityWarning, "SEVERITY_WARNING"),
        ] {
            let s = serde_json::to_string(&sev).expect("must serialize");
            assert_eq!(s, format!("\"{expected}\""));
            let back: IssueSeverity = serde_json::from_str(&s).expect("must round-trip");
            assert_eq!(back, sev);
        }
    }
}
