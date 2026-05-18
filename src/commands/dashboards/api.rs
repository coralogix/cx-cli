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

// --- API ---

const DASHBOARDS_BASE: &str = "/mgmt/openapi/5/dashboards/dashboards/v1";
const FOLDERS_BASE: &str = "/mgmt/openapi/5/dashboards/folders/v1";

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
