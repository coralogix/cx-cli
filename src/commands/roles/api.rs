use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomRole {
    pub role_id: Option<i64>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub parent_role_id: Option<i64>,
    pub parent_role_name: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub team_id: Option<i64>,
}

impl CustomRole {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_description(&self) -> &str {
        self.description.as_deref().unwrap_or("-")
    }

    pub fn permissions_count(&self) -> usize {
        self.permissions.len()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemRole {
    pub role_id: Option<i64>,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl SystemRole {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_description(&self) -> &str {
        self.description.as_deref().unwrap_or("-")
    }

    pub fn permissions_count(&self) -> usize {
        self.permissions.len()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListCustomRolesResponse {
    #[serde(default)]
    pub roles: Vec<CustomRole>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSystemRolesResponse {
    #[serde(default)]
    pub roles: Vec<SystemRole>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetCustomRoleResponse {
    pub role: Option<CustomRole>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleResponse {
    pub id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteRoleResponse {}

// --- API ---

const CUSTOM_ROLES_BASE: &str = "/mgmt/openapi/5/aaa/custom-roles/v1";
const SYSTEM_ROLES_BASE: &str = "/mgmt/openapi/5/aaa/system-roles/v1";

pub struct RolesApi<'a> {
    client: &'a CxClient,
}

impl<'a> RolesApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// List all custom roles.
    pub async fn list_custom(&self) -> Result<ListCustomRolesResponse> {
        self.client.get(CUSTOM_ROLES_BASE, &[]).await
    }

    /// Get a single custom role by ID.
    pub async fn get_custom(&self, role_id: &str) -> Result<GetCustomRoleResponse> {
        let path = format!("{CUSTOM_ROLES_BASE}/{role_id}");
        self.client.get(&path, &[]).await
    }

    /// Create a new custom role. Uses PUT per the API spec.
    pub async fn create(&self, body: &Value) -> Result<CreateRoleResponse> {
        self.client.put(CUSTOM_ROLES_BASE, body).await
    }

    /// Update a custom role. Uses POST per the API spec.
    pub async fn update(&self, role_id: &str, body: &Value) -> Result<Value> {
        let path = format!("{CUSTOM_ROLES_BASE}/{role_id}");
        self.client.post(&path, body).await
    }

    /// Delete a custom role.
    pub async fn delete(&self, role_id: &str) -> Result<DeleteRoleResponse> {
        let path = format!("{CUSTOM_ROLES_BASE}/{role_id}");
        self.client.delete(&path).await
    }

    /// List all system roles.
    pub async fn list_system(&self) -> Result<ListSystemRolesResponse> {
        self.client.get(SYSTEM_ROLES_BASE, &[]).await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_list_custom_roles() {
        let json = json!({
            "roles": [
                {
                    "roleId": 101,
                    "name": "Developer",
                    "description": "Dev access",
                    "parentRoleName": "ReadOnly",
                    "permissions": ["logs:read", "metrics:read"],
                    "teamId": 1234
                },
                {
                    "roleId": 102,
                    "name": "Admin",
                    "permissions": []
                }
            ]
        });

        let resp: ListCustomRolesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.roles.len(), 2);
        assert_eq!(resp.roles[0].display_name(), "Developer");
        assert_eq!(resp.roles[0].permissions_count(), 2);
        assert_eq!(resp.roles[1].display_name(), "Admin");
    }

    #[test]
    fn deserialize_empty_custom_roles() {
        let json = json!({});
        let resp: ListCustomRolesResponse = serde_json::from_value(json).unwrap();
        assert!(resp.roles.is_empty());
    }

    #[test]
    fn deserialize_list_system_roles() {
        let json = json!({
            "roles": [
                {
                    "roleId": 1,
                    "name": "Admin",
                    "description": "Full access",
                    "permissions": ["*"]
                }
            ]
        });

        let resp: ListSystemRolesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.roles.len(), 1);
        assert_eq!(resp.roles[0].display_name(), "Admin");
        assert_eq!(resp.roles[0].permissions_count(), 1);
    }

    #[test]
    fn deserialize_get_custom_role() {
        let json = json!({
            "role": {
                "roleId": 101,
                "name": "Developer",
                "permissions": ["logs:read"]
            }
        });
        let resp: GetCustomRoleResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.role.unwrap().display_name(), "Developer");
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({ "id": 201 });
        let resp: CreateRoleResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.id, Some(201));
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteRoleResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn display_missing_fields() {
        let role = CustomRole {
            role_id: None,
            name: None,
            description: None,
            parent_role_id: None,
            parent_role_name: None,
            permissions: vec![],
            team_id: None,
        };
        assert_eq!(role.display_name(), "-");
        assert_eq!(role.display_description(), "-");
        assert_eq!(role.permissions_count(), 0);

        let sys_role = SystemRole {
            role_id: None,
            name: None,
            description: None,
            permissions: vec![],
        };
        assert_eq!(sys_role.display_name(), "-");
        assert_eq!(sys_role.display_description(), "-");
    }
}
