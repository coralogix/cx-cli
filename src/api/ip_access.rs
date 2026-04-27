use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanyIpAccessSettings {
    pub id: Option<String>,
    pub ip_access: Option<Value>,
    pub enable_coralogix_customer_support_access: Option<Value>,
}

impl CompanyIpAccessSettings {
    pub fn display_id(&self) -> &str {
        self.id.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetIpAccessResponse {
    pub settings: Option<CompanyIpAccessSettings>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIpAccessResponse {
    pub settings: Option<CompanyIpAccessSettings>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceIpAccessResponse {
    pub settings: Option<CompanyIpAccessSettings>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteIpAccessResponse {}

// --- API ---

const IP_ACCESS_BASE: &str = "/mgmt/openapi/5/aaa/team-sec-ip-access/v1";

pub struct IpAccessApi<'a> {
    client: &'a CxClient,
}

impl<'a> IpAccessApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// Get company IP access settings.
    pub async fn get(&self) -> Result<Value> {
        self.client.get(IP_ACCESS_BASE, &[]).await
    }

    /// Create company IP access settings.
    pub async fn create(&self, body: &Value) -> Result<CreateIpAccessResponse> {
        self.client.post(IP_ACCESS_BASE, body).await
    }

    /// Replace company IP access settings.
    pub async fn replace(&self, body: &Value) -> Result<ReplaceIpAccessResponse> {
        self.client.put(IP_ACCESS_BASE, body).await
    }

    /// Delete company IP access settings.
    pub async fn delete(&self) -> Result<DeleteIpAccessResponse> {
        self.client.delete(IP_ACCESS_BASE).await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_get_response() {
        let json = json!({
            "settings": {
                "id": "d662a2f1-21c3-493c-8f8a-595d3ab05ef3",
                "ipAccess": {
                    "range1": { "ipSubnet": "10.0.0.0/8", "description": "Internal" }
                },
                "enableCoralogixCustomerSupportAccess": { "enabled": true }
            }
        });

        let resp: GetIpAccessResponse = serde_json::from_value(json).unwrap();
        let settings = resp.settings.unwrap();
        assert_eq!(settings.display_id(), "d662a2f1-21c3-493c-8f8a-595d3ab05ef3");
        assert!(settings.ip_access.is_some());
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "settings": {
                "id": "new-id",
                "ipAccess": {}
            }
        });
        let resp: CreateIpAccessResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.settings.unwrap().display_id(), "new-id");
    }

    #[test]
    fn deserialize_replace_response() {
        let json = json!({
            "settings": {
                "id": "replaced-id"
            }
        });
        let resp: ReplaceIpAccessResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.settings.unwrap().display_id(), "replaced-id");
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteIpAccessResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn display_missing_fields() {
        let settings = CompanyIpAccessSettings {
            id: None,
            ip_access: None,
            enable_coralogix_customer_support_access: None,
        };
        assert_eq!(settings.display_id(), "-");
    }
}
