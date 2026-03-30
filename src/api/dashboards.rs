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

// --- API ---

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
            .get("/mgmt/openapi/latest/dashboards/dashboards/v1/catalog", &[])
            .await
    }

    /// Get a single dashboard by ID (returns raw JSON — the schema is large).
    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("/mgmt/openapi/latest/v1/dashboards/dashboards/{id}");
        self.client.get(&path, &[]).await
    }
}
