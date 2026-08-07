use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    pub id: Option<String>,
    pub key_name: Option<String>,
    pub owner: Option<Value>,
    pub is_active: Option<bool>,
    pub created_at: Option<String>,
    pub hashed: Option<bool>,
    pub value: Option<String>,
}

impl ApiKey {
    pub fn display_name(&self) -> &str {
        self.key_name.as_deref().unwrap_or("-")
    }

    pub fn display_active(&self) -> &str {
        match self.is_active {
            Some(true) => "yes",
            Some(false) => "no",
            None => "-",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    pub id: Option<String>,
    pub name: Option<String>,
    pub owner: Option<Value>,
    pub active: Option<bool>,
    pub hashed: Option<bool>,
    pub value: Option<String>,
    pub key_permissions: Option<Value>,
}

impl KeyInfo {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_active(&self) -> &str {
        match self.active {
            Some(true) => "yes",
            Some(false) => "no",
            None => "-",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListApiKeysResponse {
    #[serde(default)]
    pub keys: Vec<KeyInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetApiKeyResponse {
    pub key_info: Option<KeyInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyResponse {
    pub key_id: Option<String>,
    pub name: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteApiKeyResponse {}

#[derive(Debug, Deserialize)]
pub struct DeleteApiKeysResponse {}

#[derive(Debug, Deserialize)]
pub struct UpdateApiKeysStatusResponse {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeySummary {
    pub api_key: Option<ApiKey>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub presets: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTeamMembersApiKeysResponse {
    #[serde(default)]
    pub keys: Vec<ApiKeySummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSendDataApiKeysResponse {
    #[serde(default)]
    pub keys: Vec<KeyInfo>,
}

// --- API ---

const API_KEYS_BASE: &str = "/mgmt/openapi/5/aaa/api-keys/v3";
const SEND_DATA_KEYS_BASE: &str = "/mgmt/openapi/5/aaa/send-data-keys/v3";

pub struct ApiKeysApi<'a> {
    client: &'a CxClient,
}

impl<'a> ApiKeysApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// List the authenticated team's API keys (`ApiKeysService_GetApiKeys`).
    ///
    /// Must be `/list/all`: `/list` collides with the `GET {base}/{key_id}`
    /// route and the server rejects it with `key_id invalid character`.
    pub async fn list(&self) -> Result<ListApiKeysResponse> {
        let path = format!("{API_KEYS_BASE}/list/all");
        self.client.get(&path, &[]).await
    }

    /// Get a single API key by ID.
    pub async fn get(&self, key_id: &str) -> Result<Value> {
        let path = format!("{API_KEYS_BASE}/{key_id}");
        self.client.get(&path, &[]).await
    }

    /// Create a new API key.
    pub async fn create(&self, body: &Value) -> Result<CreateApiKeyResponse> {
        self.client.post(API_KEYS_BASE, body).await
    }

    /// Update an existing API key.
    pub async fn update(&self, key_id: &str, body: &Value) -> Result<Value> {
        let path = format!("{API_KEYS_BASE}/{key_id}");
        self.client.put(&path, body).await
    }

    /// Delete an API key.
    pub async fn delete(&self, key_id: &str) -> Result<DeleteApiKeyResponse> {
        let path = format!("{API_KEYS_BASE}/{key_id}");
        self.client.delete(&path).await
    }

    /// Get send-data API keys.
    pub async fn get_send_data_keys(&self) -> Result<GetSendDataApiKeysResponse> {
        self.client.get(SEND_DATA_KEYS_BASE, &[]).await
    }

    /// Admin: get all team members' API keys.
    pub async fn get_team_members_keys(&self) -> Result<GetTeamMembersApiKeysResponse> {
        self.client.get(API_KEYS_BASE, &[]).await
    }

    /// Admin: bulk delete API keys.
    pub async fn bulk_delete(&self, body: &Value) -> Result<DeleteApiKeysResponse> {
        let path = format!("{API_KEYS_BASE}/all/delete");
        self.client.post(&path, body).await
    }

    /// Admin: update status of API keys.
    pub async fn update_status(&self, body: &Value) -> Result<UpdateApiKeysStatusResponse> {
        let path = format!("{API_KEYS_BASE}/all/status");
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
            "keys": [
                {
                    "id": "key-001",
                    "name": "my_api_key",
                    "active": true,
                    "hashed": true
                },
                {
                    "id": "key-002",
                    "name": "another_key",
                    "active": false
                }
            ]
        });

        let resp: ListApiKeysResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.keys.len(), 2);
        assert_eq!(resp.keys[0].display_name(), "my_api_key");
        assert_eq!(resp.keys[0].display_active(), "yes");
        assert_eq!(resp.keys[1].display_active(), "no");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListApiKeysResponse = serde_json::from_value(json).unwrap();
        assert!(resp.keys.is_empty());
    }

    #[test]
    fn deserialize_get_response() {
        let json = json!({
            "keyInfo": {
                "id": "key-001",
                "name": "my_api_key",
                "active": true
            }
        });
        let resp: GetApiKeyResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.key_info.unwrap().id.as_deref(), Some("key-001"));
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "keyId": "key-new",
            "name": "new_key",
            "value": "secret-value"
        });
        let resp: CreateApiKeyResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.key_id.as_deref(), Some("key-new"));
        assert_eq!(resp.value.as_deref(), Some("secret-value"));
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteApiKeyResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn deserialize_team_members_keys() {
        let json = json!({
            "keys": [
                {
                    "apiKey": {
                        "id": "key-001",
                        "keyName": "team_key",
                        "isActive": true
                    },
                    "permissions": ["logs:read"],
                    "presets": []
                }
            ]
        });
        let resp: GetTeamMembersApiKeysResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.keys.len(), 1);
        assert_eq!(
            resp.keys[0].api_key.as_ref().unwrap().display_name(),
            "team_key"
        );
    }

    #[test]
    fn deserialize_send_data_keys() {
        let json = json!({
            "keys": [
                { "id": "sdk-001", "name": "ingest_key", "active": true }
            ]
        });
        let resp: GetSendDataApiKeysResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.keys.len(), 1);
        assert_eq!(resp.keys[0].display_name(), "ingest_key");
    }

    #[test]
    fn display_missing_fields() {
        let key = KeyInfo {
            id: None,
            name: None,
            owner: None,
            active: None,
            hashed: None,
            value: None,
            key_permissions: None,
        };
        assert_eq!(key.display_name(), "-");
        assert_eq!(key.display_active(), "-");
    }
}
