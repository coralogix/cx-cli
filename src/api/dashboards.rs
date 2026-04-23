use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardFolderItem {
    pub id: Option<FolderIdValue>,
    pub name: Option<String>,
    pub parent_id: Option<FolderIdValue>,
}

/// The folders API returns id/parentId as `{"value": "<uuid>"}` wrappers.
#[derive(Debug, Deserialize)]
pub struct FolderIdValue {
    pub value: Option<String>,
}

impl DashboardFolderItem {
    pub fn id_str(&self) -> Option<&str> {
        self.id.as_ref().and_then(|v| v.value.as_deref())
    }

    pub fn parent_id_str(&self) -> Option<&str> {
        self.parent_id.as_ref().and_then(|v| v.value.as_deref())
    }
}

#[derive(Debug, Deserialize)]
pub struct DashboardFoldersResponse {
    #[serde(default)]
    pub folders: Vec<DashboardFolderItem>,
}

// --- API ---

const DASHBOARDS_BASE: &str = "/mgmt/openapi/latest/dashboards/dashboards/v1";
const FOLDERS_BASE: &str = "/mgmt/openapi/latest/dashboards/folders/v1";

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
        let path = format!("/mgmt/openapi/latest/v1/dashboards/dashboards/{id}");
        self.client.get(&path, &[]).await
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
}
