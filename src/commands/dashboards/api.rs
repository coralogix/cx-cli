use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

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
            .get(&format!("{DASHBOARDS_BASE}/catalog"), &[])
            .await
    }

    /// Get a single dashboard by ID (returns raw JSON — the schema is large).
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
}
