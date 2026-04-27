use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub user_id: Option<String>,
    pub user_account_id: Option<i64>,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub status: Option<String>,
}

impl User {
    pub fn display_name(&self) -> String {
        let first = self.first_name.as_deref().unwrap_or("");
        let last = self.last_name.as_deref().unwrap_or("");
        let full = format!("{first} {last}").trim().to_string();
        if full.is_empty() {
            "-".to_string()
        } else {
            full
        }
    }

    pub fn display_username(&self) -> &str {
        self.username.as_deref().unwrap_or("-")
    }

    pub fn display_status(&self) -> &str {
        self.status.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchUsersResponse {
    #[serde(default)]
    pub users: Vec<User>,
    pub next_page_token: Option<i64>,
    pub total_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetUserResponse {
    pub user: Option<User>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUsersResponse {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUsersResponse {
    #[serde(default)]
    pub user_account_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUsersStatusesResponse {
    #[serde(default)]
    pub user_account_ids: Vec<i64>,
}

// --- API ---

const USERS_BASE: &str = "/mgmt/openapi/5/aaa/teams/v2";

pub struct UsersApi<'a> {
    client: &'a CxClient,
}

impl<'a> UsersApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// Search users in a team.
    pub async fn search(&self, team_id: &str) -> Result<SearchUsersResponse> {
        let path = format!("{USERS_BASE}/{team_id}/search");
        self.client.get(&path, &[]).await
    }

    /// Search users with query parameters.
    pub async fn search_with_params(
        &self,
        team_id: &str,
        params: &[(&str, &str)],
    ) -> Result<SearchUsersResponse> {
        let path = format!("{USERS_BASE}/{team_id}/search");
        self.client.get(&path, params).await
    }

    /// Get a single user by account ID.
    pub async fn get(&self, team_id: &str, user_account_id: &str) -> Result<Value> {
        let path = format!("{USERS_BASE}/{team_id}/members/{user_account_id}");
        self.client.get(&path, &[]).await
    }

    /// Create user(s) in a team.
    pub async fn create(&self, team_id: &str, body: &Value) -> Result<CreateUsersResponse> {
        let path = format!("{USERS_BASE}/{team_id}/members");
        self.client.post(&path, body).await
    }

    /// Update user(s) in a team.
    pub async fn update(&self, team_id: &str, body: &Value) -> Result<UpdateUsersResponse> {
        let path = format!("{USERS_BASE}/{team_id}/members");
        self.client.put(&path, body).await
    }

    /// Activate or revoke users in a team.
    pub async fn update_statuses(
        &self,
        team_id: &str,
        body: &Value,
    ) -> Result<UpdateUsersStatusesResponse> {
        let path = format!("{USERS_BASE}/{team_id}/members/status");
        self.client.patch(&path, body).await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_search_response() {
        let json = json!({
            "users": [
                {
                    "userId": "uid-001",
                    "userAccountId": 12345,
                    "username": "alice@example.com",
                    "firstName": "Alice",
                    "lastName": "Smith",
                    "status": "ACTIVE"
                },
                {
                    "userId": "uid-002",
                    "username": "bob@example.com",
                    "firstName": "Bob",
                    "status": "INACTIVE"
                }
            ],
            "nextPageToken": 100,
            "totalCount": 2
        });

        let resp: SearchUsersResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.users.len(), 2);
        assert_eq!(resp.users[0].display_name(), "Alice Smith");
        assert_eq!(resp.users[0].display_username(), "alice@example.com");
        assert_eq!(resp.users[0].display_status(), "ACTIVE");
        assert_eq!(resp.users[1].display_name(), "Bob");
        assert_eq!(resp.total_count, Some(2));
    }

    #[test]
    fn deserialize_empty_search() {
        let json = json!({ "users": [] });
        let resp: SearchUsersResponse = serde_json::from_value(json).unwrap();
        assert!(resp.users.is_empty());
    }

    #[test]
    fn deserialize_get_user() {
        let json = json!({
            "user": {
                "userId": "uid-001",
                "username": "alice@example.com",
                "firstName": "Alice",
                "lastName": "Smith",
                "status": "ACTIVE"
            }
        });
        let resp: GetUserResponse = serde_json::from_value(json).unwrap();
        let user = resp.user.unwrap();
        assert_eq!(user.display_name(), "Alice Smith");
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({});
        let _resp: CreateUsersResponse = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn deserialize_update_response() {
        let json = json!({ "userAccountIds": [123, 456] });
        let resp: UpdateUsersResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.user_account_ids.len(), 2);
    }

    #[test]
    fn deserialize_update_statuses_response() {
        let json = json!({ "userAccountIds": [789] });
        let resp: UpdateUsersStatusesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.user_account_ids.len(), 1);
    }

    #[test]
    fn display_missing_fields() {
        let user = User {
            user_id: None,
            user_account_id: None,
            username: None,
            first_name: None,
            last_name: None,
            status: None,
        };
        assert_eq!(user.display_name(), "-");
        assert_eq!(user.display_username(), "-");
        assert_eq!(user.display_status(), "-");
    }
}
