use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamlConfig {
    pub team_id: Option<i64>,
    pub idp_details: Option<Value>,
    pub idp_parameters: Option<Value>,
    pub sp_parameters: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSPParametersResponse {
    pub params: Option<Value>,
}

// --- API ---

const SAML_BASE: &str = "/mgmt/openapi/5/aaa/team-saml/v1";

pub struct SamlApi<'a> {
    client: &'a CxClient,
}

impl<'a> SamlApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// Get the full SAML configuration (IDP + SP details).
    pub async fn get_config(&self) -> Result<SamlConfig> {
        let path = format!("{SAML_BASE}/configuration");
        self.client.get(&path, &[]).await
    }

    /// Set IDP parameters.
    pub async fn set_idp_params(&self, body: &Value) -> Result<Value> {
        let path = format!("{SAML_BASE}/idp_parameters");
        self.client.post(&path, body).await
    }

    /// Get SP parameters.
    pub async fn get_sp_params(&self) -> Result<GetSPParametersResponse> {
        let path = format!("{SAML_BASE}/sp_parameters");
        self.client.get(&path, &[]).await
    }

    /// Activate or deactivate SAML.
    pub async fn set_active(&self, body: &Value) -> Result<Value> {
        let path = format!("{SAML_BASE}/active");
        self.client.post(&path, body).await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_saml_config() {
        let json = json!({
            "teamId": 1234,
            "idpDetails": {
                "enabled": true,
                "url": "https://idp.example.com/saml"
            },
            "idpParameters": {
                "certificate": "MIIC..."
            },
            "spParameters": {
                "spUrl": "https://sp.example.com",
                "entityId": "coralogix"
            }
        });

        let resp: SamlConfig = serde_json::from_value(json).unwrap();
        assert_eq!(resp.team_id, Some(1234));
        assert!(resp.idp_details.is_some());
        assert!(resp.sp_parameters.is_some());
    }

    #[test]
    fn deserialize_sp_params() {
        let json = json!({
            "params": {
                "spUrl": "https://sp.example.com",
                "entityId": "coralogix"
            }
        });
        let resp: GetSPParametersResponse = serde_json::from_value(json).unwrap();
        assert!(resp.params.is_some());
    }

    #[test]
    fn deserialize_empty_config() {
        let json = json!({});
        let resp: SamlConfig = serde_json::from_value(json).unwrap();
        assert!(resp.team_id.is_none());
        assert!(resp.idp_details.is_none());
    }
}
