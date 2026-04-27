use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub id: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub default_expression: Option<String>,
    pub filters: Option<Vec<Value>>,
    pub team_id: Option<i32>,
}

impl Scope {
    pub fn display_name(&self) -> &str {
        self.display_name.as_deref().unwrap_or("-")
    }

    pub fn display_description(&self) -> &str {
        self.description.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListScopesResponse {
    #[serde(default)]
    pub scopes: Vec<Scope>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateScopeResponse {
    pub scope: Option<Scope>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteScopeResponse {}

// --- API ---

const SCOPES_BASE: &str = "/mgmt/openapi/5/aaa/team-scopes/v1";

pub struct ScopesApi<'a> {
    client: &'a CxClient,
}

impl<'a> ScopesApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// List all team scopes.
    pub async fn list(&self) -> Result<ListScopesResponse> {
        let path = format!("{SCOPES_BASE}/all/list");
        self.client.get(&path, &[]).await
    }

    /// Create a new scope.
    pub async fn create(&self, body: &Value) -> Result<CreateScopeResponse> {
        self.client.post(SCOPES_BASE, body).await
    }

    /// Update an existing scope. Uses PUT per the API spec.
    pub async fn update(&self, body: &Value) -> Result<Value> {
        self.client.put(SCOPES_BASE, body).await
    }

    /// Delete a scope by ID.
    pub async fn delete(&self, id: &str) -> Result<DeleteScopeResponse> {
        let path = format!("{SCOPES_BASE}/{id}");
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
            "scopes": [
                {
                    "id": "60c82be2-413f-4b8e-8201-7f5c51e2ef2b",
                    "displayName": "my-scope",
                    "description": "The best scope",
                    "defaultExpression": "<v1>true",
                    "filters": [{"field": "app", "value": "prod"}],
                    "teamId": 1234
                },
                {
                    "id": "abc-def",
                    "displayName": "other-scope"
                }
            ]
        });

        let resp: ListScopesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.scopes.len(), 2);
        assert_eq!(resp.scopes[0].display_name(), "my-scope");
        assert_eq!(resp.scopes[0].display_description(), "The best scope");
        assert_eq!(resp.scopes[1].display_name(), "other-scope");
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({ "scopes": [] });
        let resp: ListScopesResponse = serde_json::from_value(json).unwrap();
        assert!(resp.scopes.is_empty());
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "scope": {
                "id": "new-scope-id",
                "displayName": "new-scope"
            }
        });
        let resp: CreateScopeResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.scope.unwrap().display_name(), "new-scope");
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({});
        let _resp: DeleteScopeResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn display_missing_fields() {
        let scope = Scope {
            id: None,
            display_name: None,
            description: None,
            default_expression: None,
            filters: None,
            team_id: None,
        };
        assert_eq!(scope.display_name(), "-");
        assert_eq!(scope.display_description(), "-");
    }
}
