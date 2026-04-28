use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;
use super::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub folder_id: Option<String>,
    pub created_at: Option<String>,
}

impl View {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListViewsResponse {
    #[serde(default)]
    pub views: Vec<View>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateViewResponse {
    pub view: Option<View>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteViewResponse {}

// --- Folder response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewFolder {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub parent_id: Option<String>,
}

impl ViewFolder {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListViewFoldersResponse {
    #[serde(default)]
    pub folders: Vec<ViewFolder>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateViewFolderResponse {
    pub folder: Option<ViewFolder>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteViewFolderResponse {}

// --- API ---

const VIEWS_BASE: &str = "/mgmt/openapi/5/data-exploration/views/v1/views";
const VIEWS_FOLDERS_BASE: &str = "/mgmt/openapi/5/data-exploration/views/v1/folders";

pub struct ViewsApi<'a> {
    client: &'a CxClient,
}

impl<'a> ViewsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    // --- Views ---

    pub async fn list(&self) -> Result<ListViewsResponse> {
        self.client.get(VIEWS_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{VIEWS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateViewResponse> {
        self.client.post(VIEWS_BASE, body).await
    }

    pub async fn replace(&self, id: &str, body: &Value) -> Result<Value> {
        let path = format!("{VIEWS_BASE}/{id}");
        self.client.put(&path, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteViewResponse> {
        let path = format!("{VIEWS_BASE}/{id}");
        self.client.delete(&path).await
    }

    // --- Folders ---

    pub async fn list_folders(&self) -> Result<ListViewFoldersResponse> {
        self.client.get(VIEWS_FOLDERS_BASE, &[]).await
    }

    pub async fn get_folder(&self, id: &str) -> Result<Value> {
        let path = format!("{VIEWS_FOLDERS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create_folder(&self, body: &Value) -> Result<CreateViewFolderResponse> {
        self.client.post(VIEWS_FOLDERS_BASE, body).await
    }

    pub async fn replace_folder(&self, id: &str, body: &Value) -> Result<Value> {
        let path = format!("{VIEWS_FOLDERS_BASE}/{id}");
        self.client.put(&path, body).await
    }

    pub async fn delete_folder(&self, id: &str) -> Result<DeleteViewFolderResponse> {
        let path = format!("{VIEWS_FOLDERS_BASE}/{id}");
        self.client.delete(&path).await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_list_response() {
        let json = json!({
            "views": [
                { "id": "v-001", "name": "My View", "folderId": "f-001", "createdAt": "2024-01-01" }
            ]
        });
        let resp: ListViewsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.views.len(), 1);
        assert_eq!(resp.views[0].display_name(), "My View");
        assert_eq!(resp.views[0].folder_id.as_deref(), Some("f-001"));
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({ "views": [] });
        let resp: ListViewsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.views.is_empty());
    }

    #[test]
    fn deserialize_folder_list() {
        let json = json!({
            "folders": [
                { "id": "f-001", "name": "Infra", "parentId": null }
            ]
        });
        let resp: ListViewFoldersResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.folders.len(), 1);
        assert_eq!(resp.folders[0].display_name(), "Infra");
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({ "view": { "id": "v-001", "name": "My View" } });
        let resp: CreateViewResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.view.unwrap().id.as_deref(), Some("v-001"));
    }

    #[test]
    fn display_missing_fields() {
        let v = View {
            id: None,
            name: None,
            folder_id: None,
            created_at: None,
        };
        assert_eq!(v.display_name(), "-");
        let f = ViewFolder {
            id: None,
            name: None,
            parent_id: None,
        };
        assert_eq!(f.display_name(), "-");
    }
}
