use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Response types ---

/// The Team Groups API documents numeric IDs (e.g. `groupId: 101`) but some responses send
/// them as strings; accept either so deserialization doesn't break on conforming responses.
fn deserialize_opt_id_string<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum IdValue {
        String(String),
        Number(i64),
    }

    Ok(
        Option::<IdValue>::deserialize(deserializer)?.map(|v| match v {
            IdValue::String(s) => s,
            IdValue::Number(n) => n.to_string(),
        }),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamGroup {
    #[serde(default, deserialize_with = "deserialize_opt_id_string")]
    pub group_id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub team_id: Option<i64>,
    pub role: Option<Value>,
    pub scope: Option<Value>,
    pub group_type: Option<String>,
    pub group_origin: Option<String>,
    pub external_id: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl TeamGroup {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("-")
    }

    pub fn display_description(&self) -> &str {
        self.description.as_deref().unwrap_or("-")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTeamGroupsResponse {
    #[serde(default)]
    pub groups: Vec<TeamGroup>,
    pub next_page_token: Option<String>,
    pub total_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTeamGroupResponse {
    pub group: Option<TeamGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTeamGroupByNameResponse {
    pub group: Option<TeamGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTeamGroupResponse {
    pub group: Option<TeamGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTeamGroupResponse {
    pub group: Option<TeamGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTeamGroupResponse {
    #[serde(default, deserialize_with = "deserialize_opt_id_string")]
    pub group_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetGroupUsersResponse {
    #[serde(default)]
    pub users: Vec<Value>,
    pub next_page_token: Option<String>,
    pub total_count: Option<i64>,
}

// --- API ---

const TEAM_GROUPS_BASE: &str = "/mgmt/openapi/5/aaa/team-groups/v2";

pub struct TeamGroupsApi<'a> {
    client: &'a CxClient,
}

impl<'a> TeamGroupsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// List all team groups.
    pub async fn list(&self) -> Result<ListTeamGroupsResponse> {
        self.client.get(TEAM_GROUPS_BASE, &[]).await
    }

    /// Get a team group by ID.
    pub async fn get_by_id(&self, group_id: &str) -> Result<GetTeamGroupResponse> {
        let path = format!("{TEAM_GROUPS_BASE}/id/{group_id}");
        self.client.get(&path, &[]).await
    }

    /// Get a team group by name.
    pub async fn get_by_name(&self, name: &str) -> Result<GetTeamGroupByNameResponse> {
        let path = format!("{TEAM_GROUPS_BASE}/name/{name}");
        self.client.get(&path, &[]).await
    }

    /// Get users in a team group.
    pub async fn get_users(&self, group_id: &str) -> Result<GetGroupUsersResponse> {
        let path = format!("{TEAM_GROUPS_BASE}/{group_id}/users");
        self.client.get(&path, &[]).await
    }

    /// Create a new team group.
    pub async fn create(&self, body: &Value) -> Result<CreateTeamGroupResponse> {
        self.client.post(TEAM_GROUPS_BASE, body).await
    }

    /// Update a team group.
    pub async fn update(&self, group_id: &str, body: &Value) -> Result<UpdateTeamGroupResponse> {
        let path = format!("{TEAM_GROUPS_BASE}/{group_id}");
        self.client.put(&path, body).await
    }

    /// Delete a team group.
    pub async fn delete(&self, group_id: &str) -> Result<DeleteTeamGroupResponse> {
        let path = format!("{TEAM_GROUPS_BASE}/{group_id}");
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
            "groups": [
                {
                    "groupId": "101",
                    "name": "Engineering",
                    "description": "Eng team",
                    "teamId": 1234,
                    "createdAt": "2024-01-01T00:00:00Z"
                },
                {
                    "groupId": "102",
                    "name": "DevOps"
                }
            ],
            "totalCount": 2
        });

        let resp: ListTeamGroupsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.groups.len(), 2);
        assert_eq!(resp.groups[0].display_name(), "Engineering");
        assert_eq!(resp.groups[0].display_description(), "Eng team");
        assert_eq!(resp.groups[1].display_name(), "DevOps");
        assert_eq!(resp.total_count, Some(2));
    }

    #[test]
    fn deserialize_empty_list() {
        let json = json!({});
        let resp: ListTeamGroupsResponse = serde_json::from_value(json).unwrap();
        assert!(resp.groups.is_empty());
    }

    #[test]
    fn deserialize_get_response() {
        let json = json!({
            "group": {
                "groupId": "101",
                "name": "Engineering",
                "description": "Eng team"
            }
        });
        let resp: GetTeamGroupResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.group.unwrap().display_name(), "Engineering");
    }

    #[test]
    fn deserialize_get_by_name_response() {
        let json = json!({
            "group": {
                "groupId": "101",
                "name": "Engineering"
            }
        });
        let resp: GetTeamGroupByNameResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.group.unwrap().display_name(), "Engineering");
    }

    #[test]
    fn deserialize_create_response() {
        let json = json!({
            "group": { "groupId": "201", "name": "New Group" }
        });
        let resp: CreateTeamGroupResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.group.unwrap().group_id, Some("201".to_string()));
    }

    #[test]
    fn deserialize_update_response() {
        let json = json!({
            "group": { "groupId": "101", "name": "Updated Group" }
        });
        let resp: UpdateTeamGroupResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.group.unwrap().display_name(), "Updated Group");
    }

    #[test]
    fn deserialize_delete_response() {
        let json = json!({ "groupId": "101" });
        let resp: DeleteTeamGroupResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.group_id, Some("101".to_string()));
    }

    #[test]
    fn deserialize_numeric_group_ids() {
        let json = json!({
            "groups": [
                { "groupId": 101, "name": "Engineering" }
            ]
        });
        let resp: ListTeamGroupsResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.groups[0].group_id, Some("101".to_string()));

        let delete_json = json!({ "groupId": 101 });
        let resp: DeleteTeamGroupResponse = serde_json::from_value(delete_json).unwrap();
        assert_eq!(resp.group_id, Some("101".to_string()));
    }

    #[test]
    fn deserialize_group_users_response() {
        let json = json!({
            "users": [
                { "userId": "uid-001", "username": "alice@example.com" }
            ],
            "totalCount": 1
        });
        let resp: GetGroupUsersResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.users.len(), 1);
        assert_eq!(resp.total_count, Some(1));
    }

    #[test]
    fn display_missing_fields() {
        let group = TeamGroup {
            group_id: None,
            name: None,
            description: None,
            team_id: None,
            role: None,
            scope: None,
            group_type: None,
            group_origin: None,
            external_id: None,
            created_at: None,
            updated_at: None,
        };
        assert_eq!(group.display_name(), "-");
        assert_eq!(group.display_description(), "-");
    }
}
