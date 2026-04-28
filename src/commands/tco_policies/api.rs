use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;
use crate::serde_helpers::string_or_number;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TcoPolicy {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub name: Option<String>,
    pub priority: Option<String>,
    pub source_type: Option<String>,
    pub severity: Option<String>,
    pub archive_retention: Option<Value>,
    pub enabled: Option<bool>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub application_rule: Option<Value>,
    pub subsystem_rule: Option<Value>,
}

impl TcoPolicy {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_priority(&self) -> String {
        self.priority
            .as_deref()
            .map(|s| s.strip_prefix("PRIORITY_TYPE_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn display_source_type(&self) -> String {
        self.source_type
            .as_deref()
            .map(|s| s.strip_prefix("SOURCE_TYPE_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn display_severity(&self) -> String {
        self.severity
            .as_deref()
            .map(|s| s.strip_prefix("SEVERITY_").unwrap_or(s).to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn display_archive_retention(&self) -> String {
        self.archive_retention
            .as_ref()
            .and_then(|v| v.get("id").and_then(|id| id.as_str()))
            .unwrap_or("-")
            .to_string()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTcoPoliciesResponse {
    #[serde(default)]
    pub policies: Vec<TcoPolicy>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTcoPolicyResponse {
    pub policy: Option<TcoPolicy>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTcoPolicyResponse {
    pub policy: Option<TcoPolicy>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTcoPolicyResponse {
    pub policy: Option<TcoPolicy>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteTcoPolicyResponse {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TcoSettingsResponse {
    #[serde(default)]
    pub settings: Value,
}

// --- API ---

const TCO_POLICIES_BASE: &str = "/mgmt/openapi/5/dataplans/policies/v1";
const TCO_POLICY_SETTINGS_BASE: &str = "/mgmt/openapi/5/dataplans/policy-settings/v1";

pub struct TcoPoliciesApi<'a> {
    client: &'a CxClient,
}

impl<'a> TcoPoliciesApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list(&self) -> Result<ListTcoPoliciesResponse> {
        self.client.get(TCO_POLICIES_BASE, &[]).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let path = format!("{TCO_POLICIES_BASE}/{id}");
        self.client.get(&path, &[]).await
    }

    pub async fn create(&self, body: &Value) -> Result<CreateTcoPolicyResponse> {
        self.client.post(TCO_POLICIES_BASE, body).await
    }

    pub async fn update(&self, body: &Value) -> Result<UpdateTcoPolicyResponse> {
        self.client.put(TCO_POLICIES_BASE, body).await
    }

    pub async fn delete(&self, id: &str) -> Result<DeleteTcoPolicyResponse> {
        let path = format!("{TCO_POLICIES_BASE}/{id}");
        self.client.delete(&path).await
    }

    pub async fn reorder(&self, body: &Value) -> Result<Value> {
        let path = format!("{TCO_POLICIES_BASE}/all/reorder");
        self.client.post(&path, body).await
    }

    pub async fn test_policies(&self, body: &Value) -> Result<Value> {
        let path = format!("{TCO_POLICIES_BASE}/all/test-policies");
        self.client.post(&path, body).await
    }

    pub async fn get_settings(&self) -> Result<Value> {
        self.client.get(TCO_POLICY_SETTINGS_BASE, &[]).await
    }

    pub async fn replace_settings(&self, body: &Value) -> Result<Value> {
        self.client.put(TCO_POLICY_SETTINGS_BASE, body).await
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
            "policies": [
                {
                    "id": "policy-001",
                    "name": "Production Logs",
                    "priority": "PRIORITY_TYPE_HIGH",
                    "sourceType": "SOURCE_TYPE_LOGS",
                    "severity": "SEVERITY_INFO",
                    "enabled": true,
                    "createdAt": "2024-01-01T00:00:00Z",
                    "archiveRetention": {"id": "ret-001"}
                },
                {
                    "id": "policy-002",
                    "name": "Debug Spans",
                    "priority": "PRIORITY_TYPE_LOW",
                    "sourceType": "SOURCE_TYPE_SPANS",
                    "severity": "SEVERITY_DEBUG",
                    "enabled": false
                }
            ]
        });

        let resp: ListTcoPoliciesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.policies.len(), 2);
        assert_eq!(resp.policies[0].id.as_deref(), Some("policy-001"));
        assert_eq!(resp.policies[0].display_name(), "Production Logs");
        assert_eq!(resp.policies[0].display_priority(), "HIGH");
        assert_eq!(resp.policies[0].display_source_type(), "LOGS");
        assert_eq!(resp.policies[0].display_severity(), "INFO");
        assert_eq!(resp.policies[0].display_archive_retention(), "ret-001");
        assert_eq!(resp.policies[0].enabled, Some(true));
        assert_eq!(resp.policies[1].display_priority(), "LOW");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListTcoPoliciesResponse = serde_json::from_value(json).unwrap();
        assert!(resp.policies.is_empty());
    }

    #[test]
    fn deserialize_get_response() {
        let json = json!({
            "policy": {
                "id": "policy-001",
                "name": "Production Logs",
                "priority": "PRIORITY_TYPE_HIGH"
            }
        });
        let resp: GetTcoPolicyResponse = serde_json::from_value(json).unwrap();
        let policy = resp.policy.unwrap();
        assert_eq!(policy.id.as_deref(), Some("policy-001"));
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "policy": {
                "id": "policy-new",
                "name": "New Policy"
            }
        });
        let resp: CreateTcoPolicyResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.policy.unwrap().id.as_deref(), Some("policy-new"));
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteTcoPolicyResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn display_missing_fields() {
        let policy = TcoPolicy {
            id: None,
            name: None,
            priority: None,
            source_type: None,
            severity: None,
            archive_retention: None,
            enabled: None,
            created_at: None,
            updated_at: None,
            application_rule: None,
            subsystem_rule: None,
        };
        assert_eq!(policy.display_name(), "-");
        assert_eq!(policy.display_priority(), "-");
        assert_eq!(policy.display_source_type(), "-");
        assert_eq!(policy.display_severity(), "-");
        assert_eq!(policy.display_archive_retention(), "-");
    }
}
