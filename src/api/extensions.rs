use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extension {
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub deployed: Option<bool>,
    pub updated: Option<String>,
}

impl Extension {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_version(&self) -> &str {
        self.version.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListExtensionsResponse {
    #[serde(default)]
    pub extensions: Vec<Extension>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDeployedExtensionsResponse {
    #[serde(default)]
    pub deployed_extensions: Vec<Extension>,
}

// --- API ---

const EXTENSIONS_BASE: &str = "/mgmt/openapi/latest/extensions/extensions/v1";

pub struct ExtensionsApi<'a> {
    client: &'a CxClient,
}

impl<'a> ExtensionsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list_all(&self) -> Result<ListExtensionsResponse> {
        self.client.get(EXTENSIONS_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{EXTENSIONS_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn list_deployed(&self) -> Result<ListDeployedExtensionsResponse> {
        let path = format!("{EXTENSIONS_BASE}/deployed");
        self.client.get(&path, &[]).await
    }

    pub async fn deploy(&self, body: &Value) -> Result<Value> {
        let path = format!("{EXTENSIONS_BASE}/deploy");
        self.client.post(&path, body).await
    }

    pub async fn update(&self, body: &Value) -> Result<Value> {
        let path = format!("{EXTENSIONS_BASE}/update");
        self.client.post(&path, body).await
    }

    pub async fn undeploy(&self, body: &Value) -> Result<Value> {
        let path = format!("{EXTENSIONS_BASE}/undeploy");
        self.client.post(&path, body).await
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
            "extensions": [
                { "id": "ext-001", "name": "AWS CloudWatch", "version": "1.0.0", "deployed": false }
            ]
        });
        let resp: ListExtensionsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.extensions.len(), 1);
        assert_eq!(resp.extensions[0].display_name(), "AWS CloudWatch");
        assert_eq!(resp.extensions[0].display_version(), "1.0.0");
    }

    #[test]
    fn deserialize_deployed_response() {
        let json = json!({ "deployedExtensions": [] });
        let resp: ListDeployedExtensionsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.deployed_extensions.is_empty());
    }

    #[test]
    fn display_missing_fields() {
        let e = Extension {
            id: None,
            name: None,
            version: None,
            deployed: None,
            updated: None,
        };
        assert_eq!(e.display_name(), "-");
        assert_eq!(e.display_version(), "-");
    }
}
