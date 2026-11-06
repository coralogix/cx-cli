use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;
use crate::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extension {
    #[serde(default, deserialize_with = "string_or_number")]
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

// The deployed endpoint returns a different shape than the catalog: items
// have no name/deployed/updated, but carry deployment scope and a summary
// of deployed item counts.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployedExtension {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub applications: Vec<String>,
    #[serde(default)]
    pub subsystems: Vec<String>,
    #[serde(default)]
    pub item_ids: Vec<String>,
    pub summary: Option<Value>,
}

impl DeployedExtension {
    /// Total number of deployed items, summed from
    /// `summary.deployedItemCounts`; falls back to the length of `itemIds`
    /// when the summary is absent.
    pub fn deployed_item_count(&self) -> Option<u64> {
        let from_summary = self
            .summary
            .as_ref()
            .and_then(|s| s.get("deployedItemCounts"))
            .and_then(Value::as_object)
            .map(|counts| {
                counts
                    .values()
                    .filter_map(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    })
                    .sum()
            });
        match from_summary {
            Some(n) => Some(n),
            None if !self.item_ids.is_empty() => Some(self.item_ids.len() as u64),
            None => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDeployedExtensionsResponse {
    #[serde(default)]
    pub deployed_extensions: Vec<DeployedExtension>,
}

// --- API ---

const EXTENSIONS_BASE: &str = "/mgmt/openapi/5/integrations/extensions/v1";

pub struct ExtensionsApi<'a> {
    client: &'a CxClient,
}

impl<'a> ExtensionsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list_all(&self) -> Result<ListExtensionsResponse> {
        self.client
            .post(EXTENSIONS_BASE, &serde_json::json!({}))
            .await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{EXTENSIONS_BASE}/catalog/{id}");
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
        // Realistic payload shape for GET .../extensions/v1/deployed: items
        // carry id/version/scope/summary but no name/deployed/updated.
        let json = json!({
            "deployedExtensions": [
                {
                    "id": "K8sObservability",
                    "version": "1.0.3",
                    "applications": ["prod-eu", "staging"],
                    "subsystems": ["kube-system"],
                    "itemIds": ["alert-1", "dash-1", "dash-2"],
                    "summary": {
                        "deployedItemCounts": { "alerts": 1, "grafanaDashboards": 2 }
                    }
                },
                { "id": "CoralogixSystem", "version": "0.2.1" }
            ]
        });
        let resp: ListDeployedExtensionsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.deployed_extensions.len(), 2);

        let full = &resp.deployed_extensions[0];
        assert_eq!(full.id.as_deref(), Some("K8sObservability"));
        assert_eq!(full.version.as_deref(), Some("1.0.3"));
        assert_eq!(full.applications, vec!["prod-eu", "staging"]);
        assert_eq!(full.subsystems, vec!["kube-system"]);
        assert_eq!(full.deployed_item_count(), Some(3));

        let sparse = &resp.deployed_extensions[1];
        assert_eq!(sparse.id.as_deref(), Some("CoralogixSystem"));
        assert!(sparse.applications.is_empty());
        assert_eq!(sparse.deployed_item_count(), None);
    }

    #[test]
    fn deployed_item_count_handles_string_counts_and_item_ids_fallback() {
        // Proto int64 fields can serialize as JSON strings.
        let json = json!({
            "id": "Ext",
            "summary": { "deployedItemCounts": { "alerts": "4", "savedViews": 1 } }
        });
        let ext: DeployedExtension = serde_json::from_value(json).unwrap();
        assert_eq!(ext.deployed_item_count(), Some(5));

        let json = json!({ "id": "Ext", "itemIds": ["a", "b"] });
        let ext: DeployedExtension = serde_json::from_value(json).unwrap();
        assert_eq!(ext.deployed_item_count(), Some(2));
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
